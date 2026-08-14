//! Phase 9 integration smokes: multi-message GEM session + optional Secs4Net interop.
//!
//! Source goal: `PORTING_LEDGER` Phase 9 (Active↔Passive + C# interop smoke).

#[cfg(test)]
mod tests {
    use crate::gem::{
        s1f1, s1f13, s1f14, s1f17, s1f18, s1f2, s2f17, s2f18, s5f1_alarm, s5f2, s6f11_empty, s6f12,
        Ackc5, Ackc6, Clock, ClockType, CommAck, GemConfig, LocalDateTime, OnlAck,
    };
    use crate::hsms::{HsmsCommunicateState, HsmsConnectionMode};
    use crate::hsms_ss::{HsmsSsCommunicator, HsmsSsCommunicatorConfig};
    use crate::secs2::Secs2;
    use crate::SecsMessage;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn open_pair() -> (Arc<HsmsSsCommunicator>, Arc<HsmsSsCommunicator>) {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let p_cfg = HsmsSsCommunicatorConfig::new();
        p_cfg.set_session_id(10).unwrap();
        p_cfg.set_connection_mode(HsmsConnectionMode::Passive);
        p_cfg.set_socket_address(addr);
        p_cfg.timeout().set_t7(5.0);
        let passive = Arc::new(HsmsSsCommunicator::new_instance(p_cfg));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(5.0);
        a_cfg.timeout().set_t6(5.0);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));

        let p_arc = Arc::clone(&passive);
        let p = thread::spawn(move || p_arc.open_passive().unwrap());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !passive.is_open() {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        active.open_active().unwrap();
        p.join().unwrap();
        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );
        (active, passive)
    }

    /// Multi-message GEM session over one Active↔Passive TCP link.
    #[test]
    fn phase9_gem_session_smoke_rust_rust() {
        let (host, equip) = open_pair();

        // Equip side: answer S1/S2/S5/S6 primaries for the session.
        let eq = Arc::clone(&equip);
        let worker = thread::spawn(move || {
            let pcfg = GemConfig::new();
            pcfg.set_is_equip(true);
            pcfg.set_mdln("EQ-SMOKE");
            pcfg.set_softrev("0.1");
            pcfg.set_clock_type(ClockType::A16);
            let clock = Clock::from_local(LocalDateTime {
                year: 2026,
                month: 8,
                day: 5,
                hour: 10,
                minute: 20,
                second: 30,
                hundredths: 40,
            });

            // S1F13
            let m = eq.take_data_message().unwrap();
            assert_eq!((m.get_stream(), m.get_function()), (1, 13));
            assert!(s1f14(&eq, &m, &pcfg, CommAck::Ok).unwrap());

            // S1F17
            let m = eq.take_data_message().unwrap();
            assert_eq!((m.get_stream(), m.get_function()), (1, 17));
            assert!(s1f18(&eq, &m, OnlAck::Ok).unwrap());

            // S1F1
            let m = eq.take_data_message().unwrap();
            assert_eq!((m.get_stream(), m.get_function()), (1, 1));
            assert!(s1f2(&eq, &m, &pcfg).unwrap());

            // S2F17
            let m = eq.take_data_message().unwrap();
            assert_eq!((m.get_stream(), m.get_function()), (2, 17));
            assert!(s2f18(&eq, &m, &pcfg, &clock).unwrap());

            // S6F11 (host as equipment event sink)
            let m = eq.take_data_message().unwrap();
            assert_eq!((m.get_stream(), m.get_function()), (6, 11));
            assert!(s6f12(&eq, &m, Ackc6::Ok).unwrap());

            // S5F1
            let m = eq.take_data_message().unwrap();
            assert_eq!((m.get_stream(), m.get_function()), (5, 1));
            assert!(s5f2(&eq, &m, Ackc5::Ok).unwrap());
        });

        // Host sequence
        let hcfg = GemConfig::new(); // host: empty MDLN list
        assert_eq!(s1f13(&host, &hcfg).unwrap(), CommAck::Ok);
        assert_eq!(s1f17(&host).unwrap(), OnlAck::Ok);

        let s1f2 = s1f1(&host).unwrap().expect("S1F2");
        assert_eq!(s1f2.get_function(), 2);
        assert_eq!(s1f2.secs2().get_ascii_at(&[0]).unwrap(), "EQ-SMOKE");
        assert_eq!(s1f2.secs2().get_ascii_at(&[1]).unwrap(), "0.1");

        let clock = s2f17(&host).unwrap();
        assert_eq!(clock.to_local_date_time().year, 2026);
        assert_eq!(clock.to_local_date_time().minute, 20);

        let ack6 = s6f11_empty(
            &host,
            Secs2::uint4([1]).unwrap(),
            Secs2::uint2([100]).unwrap(),
        )
        .unwrap();
        assert_eq!(ack6, Ackc6::Ok);

        let ack5 = s5f1_alarm(&host, 0x81, 1001, "SMOKE").unwrap();
        assert_eq!(ack5, Ackc5::Ok);

        worker.join().unwrap();
        host.close();
        equip.close();
    }

    fn interop_csproj() -> PathBuf {
        // crates/secs4rs → secs4rs/interop/csharp_hsms_passive
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../interop/csharp_hsms_passive/csharp_hsms_passive.csproj")
    }

    /// secs4rs Active host ↔ Secs4Net (C#) Passive equipment.
    #[test]
    fn phase9_csharp_hsms_ss_s1_interop() {
        let csproj = interop_csproj();
        if !csproj.is_file() {
            eprintln!("skip: missing {csproj:?}");
            return;
        }
        // Ensure C# helper is buildable (uses workspace Secs4Net).
        let build = Command::new("dotnet")
            .args(["build", "-c", "Release", "--nologo", "-v", "q"])
            .arg(&csproj)
            .output()
            .expect("dotnet build spawn");
        if !build.status.success() {
            panic!(
                "dotnet build failed:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
        }

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        let port = addr.port();
        drop(probe);

        let proj_dir = csproj.parent().expect("csproj dir");
        let mut child = Command::new("dotnet")
            .args([
                "run",
                "-c",
                "Release",
                "--project",
                csproj.to_str().unwrap(),
                "--no-build",
                "--",
                &port.to_string(),
                "10",
            ])
            .current_dir(proj_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("dotnet run spawn");

        // Wait for READY
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let ready_deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let stderr = {
                        use std::io::Read;
                        let mut s = String::new();
                        if let Some(mut e) = child.stderr.take() {
                            let _ = e.read_to_string(&mut s);
                        }
                        s
                    };
                    let status = child.wait().ok();
                    panic!("C# process exited before READY: status={status:?} stderr={stderr}");
                }
                Ok(_) => {
                    if line.trim() == "READY" {
                        break;
                    }
                }
                Err(e) => panic!("read READY: {e}"),
            }
            assert!(
                std::time::Instant::now() < ready_deadline,
                "timeout waiting C# READY"
            );
        }

        // Accept loop is async after Open(); do not TCP-probe (would steal SS connection).
        thread::sleep(Duration::from_millis(200));

        // Rust Active host → C# Passive equip
        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(10.0);
        a_cfg.timeout().set_t6(5.0);
        a_cfg.timeout().set_t5(0.2);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));
        // Background T5 retry + wait until SELECTED (open_active_with_t5_retry returns immediately).
        active
            .open_active_with_t5_retry()
            .expect("open_active_with_t5_retry");
        active.wait_until_hsms_communicate_state(HsmsCommunicateState::Selected);
        assert_eq!(
            active.hsms_communicate_state(),
            HsmsCommunicateState::Selected
        );

        let hcfg = GemConfig::new();
        assert_eq!(s1f13(&active, &hcfg).unwrap(), CommAck::Ok);
        assert_eq!(s1f17(&active).unwrap(), OnlAck::Ok);

        let reply = s1f1(&active).unwrap().expect("S1F2 from C#");
        assert_eq!(reply.get_stream(), 1);
        assert_eq!(reply.get_function(), 2);
        // C# equip MDLN/SOFTREV
        assert_eq!(reply.secs2().get_ascii_at(&[0]).unwrap(), "CS-EQ");
        assert_eq!(reply.secs2().get_ascii_at(&[1]).unwrap(), "1.0.0");

        active.close();
        let _ = child.kill();
        let _ = child.wait();
    }

    fn interop_java_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../interop/java_hsms_passive")
    }

    fn interop_java_jar() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../secs4java8/Export.jar")
    }

    /// secs4rs Active host ↔ secs4java8 (Java) Passive equipment.
    #[test]
    fn phase9_java_hsms_ss_s1_interop() {
        let java_dir = interop_java_dir();
        let jar = interop_java_jar();
        let src = java_dir.join("JavaHsmsPassive.java");
        if !src.is_file() || !jar.is_file() {
            eprintln!("skip: missing Java interop src or Export.jar");
            return;
        }

        // Compile against secs4java8 Export.jar (Java 8+ bytecode).
        let javac = Command::new("javac")
            .args([
                "-cp",
                jar.to_str().unwrap(),
                "-d",
                java_dir.to_str().unwrap(),
                src.to_str().unwrap(),
            ])
            .output()
            .expect("javac spawn");
        if !javac.status.success() {
            panic!(
                "javac failed:\n{}",
                String::from_utf8_lossy(&javac.stderr)
            );
        }

        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        let port = addr.port();
        drop(probe);

        let cp = format!("{}:{}", jar.to_str().unwrap(), java_dir.to_str().unwrap());
        let mut child = Command::new("java")
            .args([
                "-cp",
                &cp,
                "JavaHsmsPassive",
                &port.to_string(),
                "10",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("java spawn");

        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let ready_deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let stderr = {
                        use std::io::Read;
                        let mut s = String::new();
                        if let Some(mut e) = child.stderr.take() {
                            let _ = e.read_to_string(&mut s);
                        }
                        s
                    };
                    let status = child.wait().ok();
                    panic!("Java process exited before READY: status={status:?} stderr={stderr}");
                }
                Ok(_) => {
                    if line.trim() == "READY" {
                        break;
                    }
                }
                Err(e) => panic!("read READY: {e}"),
            }
            assert!(
                std::time::Instant::now() < ready_deadline,
                "timeout waiting Java READY"
            );
        }

        thread::sleep(Duration::from_millis(200));

        let a_cfg = HsmsSsCommunicatorConfig::new();
        a_cfg.set_session_id(10).unwrap();
        a_cfg.set_connection_mode(HsmsConnectionMode::Active);
        a_cfg.set_socket_address(addr);
        a_cfg.timeout().set_t3(10.0);
        a_cfg.timeout().set_t6(5.0);
        a_cfg.timeout().set_t5(0.2);
        let active = Arc::new(HsmsSsCommunicator::new_instance(a_cfg));
        active
            .open_active_with_t5_retry()
            .expect("open_active_with_t5_retry");
        active.wait_until_hsms_communicate_state(HsmsCommunicateState::Selected);

        let hcfg = GemConfig::new();
        assert_eq!(s1f13(&active, &hcfg).unwrap(), CommAck::Ok);
        assert_eq!(s1f17(&active).unwrap(), OnlAck::Ok);

        let reply = s1f1(&active).unwrap().expect("S1F2 from Java");
        assert_eq!(reply.get_stream(), 1);
        assert_eq!(reply.get_function(), 2);
        assert_eq!(reply.secs2().get_ascii_at(&[0]).unwrap(), "JV-EQ");
        assert_eq!(reply.secs2().get_ascii_at(&[1]).unwrap(), "2.0.0");

        active.close();
        let _ = child.kill();
        let _ = child.wait();
    }
}
