//! Dynamic event-report configuration registry (S2F33/35/37 body assembly).
//!
//! Source: `AbstractDynamicEventReportConfig` (add/remove/get + define/link/enable lists).
//! Network send (`S2f33Define` → DRACK) deferred until full Gem façade; this builds bodies.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use crate::secs2::{self, Secs2};

use super::dynamic::{DynamicCollectionEvent, DynamicLink, DynamicReport};

/// `DynamicEventReportConfig` — holds define-reports, links, enable-CEIDs.
///
/// ID encoding defaults to U4 (common host/equipment path). Auto RPTID increments from 1.
pub struct DynamicEventReportConfig {
    reports: Mutex<Vec<DynamicReport>>,
    links: Mutex<Vec<DynamicLink>>,
    events: Mutex<Vec<DynamicCollectionEvent>>,
    auto_report_id: AtomicI64,
    auto_data_id: AtomicI64,
}

impl Default for DynamicEventReportConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicEventReportConfig {
    pub fn new() -> Self {
        Self {
            reports: Mutex::new(Vec::new()),
            links: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            auto_report_id: AtomicI64::new(0),
            auto_data_id: AtomicI64::new(0),
        }
    }

    /// Next auto RPTID as U4 (`AbstractGem.AutoReportId` simplified → U4).
    pub fn auto_report_id_u4(&self) -> secs2::Result<Secs2> {
        let n = self.auto_report_id.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        Secs2::uint4([n as u32])
    }

    /// Next DATAID as U4 for S2F33/35 wrapper list.
    pub fn auto_data_id_u4(&self) -> secs2::Result<Secs2> {
        let n = self.auto_data_id.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        Secs2::uint4([n as u32])
    }

    fn u4(id: i64) -> secs2::Result<Secs2> {
        Secs2::uint4([id as u32])
    }

    /// `AddDefineReport(reportId, alias, vids)`.
    pub fn add_define_report(
        &self,
        report_id: i64,
        alias: Option<String>,
        vids: &[i64],
    ) -> secs2::Result<DynamicReport> {
        let rptid = Self::u4(report_id)?;
        let vv: Result<Vec<_>, _> = vids.iter().map(|v| Self::u4(*v)).collect();
        let r = DynamicReport::new(rptid, alias, vv?);
        self.reports.lock().expect("reports").push(r.clone());
        Ok(r)
    }

    /// `AddDefineReport(alias, vids)` with auto RPTID.
    pub fn add_define_report_auto(
        &self,
        alias: Option<String>,
        vids: &[i64],
    ) -> secs2::Result<DynamicReport> {
        let rptid = self.auto_report_id_u4()?;
        let vv: Result<Vec<_>, _> = vids.iter().map(|v| Self::u4(*v)).collect();
        let r = DynamicReport::new(rptid, alias, vv?);
        self.reports.lock().expect("reports").push(r.clone());
        Ok(r)
    }

    /// Remove by RPTID equality (`DynamicReport.Equals` on report id).
    pub fn remove_report(&self, report: &DynamicReport) -> bool {
        let mut g = self.reports.lock().expect("reports");
        if let Some(i) = g.iter().position(|r| r.report_id() == report.report_id()) {
            g.remove(i);
            true
        } else {
            false
        }
    }

    pub fn get_report_by_alias(&self, alias: &str) -> Option<DynamicReport> {
        self.reports
            .lock()
            .expect("reports")
            .iter()
            .find(|r| r.alias() == Some(alias))
            .cloned()
    }

    pub fn get_report_by_id(&self, report_id: &Secs2) -> Option<DynamicReport> {
        self.reports
            .lock()
            .expect("reports")
            .iter()
            .find(|r| r.report_id() == report_id)
            .cloned()
    }

    pub fn report_count(&self) -> usize {
        self.reports.lock().expect("reports").len()
    }

    /// `AddLinkByReport(ceid, reports)`.
    pub fn add_link_by_report(
        &self,
        ceid: i64,
        reports: &[DynamicReport],
    ) -> secs2::Result<DynamicLink> {
        let ce = DynamicCollectionEvent::new(None, Self::u4(ceid)?);
        let rpts: Vec<Secs2> = reports.iter().map(|r| r.report_id().clone()).collect();
        let link = DynamicLink::new(ce, rpts);
        self.links.lock().expect("links").push(link.clone());
        Ok(link)
    }

    /// `AddLinkById(ceid, reportIds)`.
    pub fn add_link_by_id(&self, ceid: i64, report_ids: &[i64]) -> secs2::Result<DynamicLink> {
        let ce = DynamicCollectionEvent::new(None, Self::u4(ceid)?);
        let rpts: Result<Vec<_>, _> = report_ids.iter().map(|id| Self::u4(*id)).collect();
        let link = DynamicLink::new(ce, rpts?);
        self.links.lock().expect("links").push(link.clone());
        Ok(link)
    }

    pub fn remove_link(&self, link: &DynamicLink) -> bool {
        let mut g = self.links.lock().expect("links");
        // Equality in source is on collection event only
        if let Some(i) = g
            .iter()
            .position(|l| l.collection_event_id() == link.collection_event_id())
        {
            g.remove(i);
            true
        } else {
            false
        }
    }

    pub fn link_count(&self) -> usize {
        self.links.lock().expect("links").len()
    }

    /// `AddEnableCollectionEvent(alias, ceid)`.
    pub fn add_enable_collection_event(
        &self,
        alias: Option<String>,
        ceid: i64,
    ) -> secs2::Result<DynamicCollectionEvent> {
        let ce = DynamicCollectionEvent::new(alias, Self::u4(ceid)?);
        self.events.lock().expect("events").push(ce.clone());
        Ok(ce)
    }

    pub fn remove_enable_collection_event(&self, ce: &DynamicCollectionEvent) -> bool {
        let mut g = self.events.lock().expect("events");
        if let Some(i) = g
            .iter()
            .position(|e| e.collection_event_id() == ce.collection_event_id())
        {
            g.remove(i);
            true
        } else {
            false
        }
    }

    pub fn get_collection_event_by_alias(&self, alias: &str) -> Option<DynamicCollectionEvent> {
        self.events
            .lock()
            .expect("events")
            .iter()
            .find(|e| e.alias() == Some(alias))
            .cloned()
    }

    pub fn event_count(&self) -> usize {
        self.events.lock().expect("events").len()
    }

    /// Snapshot of single-report items for S2F33 define list (without DATAID wrapper).
    pub fn s2f33_report_items(&self) -> secs2::Result<Vec<Secs2>> {
        let g = self.reports.lock().expect("reports");
        g.iter().map(|r| r.to_s2f33_report()).collect()
    }

    /// Full S2F33 body: `L <DATAID> <L reports…>` (`S2f33Inner`).
    pub fn s2f33_define_body(&self, data_id: Secs2) -> secs2::Result<Secs2> {
        let items = self.s2f33_report_items()?;
        let list = Secs2::list(items)?;
        Secs2::list([data_id, list])
    }

    /// S2F33 delete-all body: `L <DATAID> <L>` empty reports.
    pub fn s2f33_delete_all_body(&self, data_id: Secs2) -> secs2::Result<Secs2> {
        Secs2::list([data_id, Secs2::list_empty()])
    }

    /// Snapshot of single-link items for S2F35.
    pub fn s2f35_link_items(&self) -> secs2::Result<Vec<Secs2>> {
        let g = self.links.lock().expect("links");
        g.iter().map(|l| l.to_s2f35_link()).collect()
    }

    /// Full S2F35 body: `L <DATAID> <L links…>`.
    pub fn s2f35_body(&self, data_id: Secs2) -> secs2::Result<Secs2> {
        let items = self.s2f35_link_items()?;
        Secs2::list([data_id, Secs2::list(items)?])
    }

    /// CEID list for S2F37 enable (config events only).
    pub fn s2f37_ceid_items(&self) -> Vec<Secs2> {
        self.events
            .lock()
            .expect("events")
            .iter()
            .map(|e| e.collection_event_id().clone())
            .collect()
    }

    /// Full S2F37 enable body: `L <CEED> <L CEIDs…>` (`S2f37Inner` shape).
    pub fn s2f37_enable_body(&self, ceed: &Secs2) -> secs2::Result<Secs2> {
        let ceids = Secs2::list(self.s2f37_ceid_items())?;
        Secs2::list([ceed.clone(), ceids])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gem::Ceed;

    fn u4_at(s: &Secs2, path: &[usize]) -> i64 {
        let mut full = path.to_vec();
        full.push(0);
        s.get_long_at(&full).unwrap()
    }

    #[test]
    fn config_define_report_get_remove() {
        let cfg = DynamicEventReportConfig::new();
        let r = cfg
            .add_define_report(10, Some("RPT-A".into()), &[1, 2, 3])
            .unwrap();
        assert_eq!(cfg.report_count(), 1);
        assert_eq!(
            cfg.get_report_by_alias("RPT-A").unwrap().report_id(),
            r.report_id()
        );
        assert!(cfg.get_report_by_id(r.report_id()).is_some());
        assert!(cfg.remove_report(&r));
        assert_eq!(cfg.report_count(), 0);
        assert!(!cfg.remove_report(&r));
    }

    #[test]
    fn config_auto_report_id_increments() {
        let cfg = DynamicEventReportConfig::new();
        let r1 = cfg.add_define_report_auto(None, &[5]).unwrap();
        let r2 = cfg.add_define_report_auto(Some("B".into()), &[6]).unwrap();
        assert_ne!(r1.report_id(), r2.report_id());
        assert_eq!(u4_at(r1.report_id(), &[]), 1);
        assert_eq!(u4_at(r2.report_id(), &[]), 2);
    }

    #[test]
    fn config_s2f33_define_body_shape() {
        let cfg = DynamicEventReportConfig::new();
        cfg.add_define_report(101, None, &[1, 2]).unwrap();
        cfg.add_define_report(102, None, &[3]).unwrap();
        let data_id = Secs2::uint4([7]).unwrap();
        let body = cfg.s2f33_define_body(data_id).unwrap();
        // L <U4 7> <L <report> <report>>
        assert_eq!(body.size(), 2);
        assert_eq!(u4_at(&body, &[0]), 7);
        assert_eq!(body.get_item(&[1]).unwrap().size(), 2);
        // first report RPTID 101
        assert_eq!(u4_at(&body, &[1, 0, 0]), 101);
        assert_eq!(u4_at(&body, &[1, 0, 1, 0]), 1);
        assert_eq!(u4_at(&body, &[1, 0, 1, 1]), 2);
        assert_eq!(u4_at(&body, &[1, 1, 0]), 102);
    }

    #[test]
    fn config_s2f33_delete_all_empty_list() {
        let cfg = DynamicEventReportConfig::new();
        let body = cfg
            .s2f33_delete_all_body(Secs2::uint4([1]).unwrap())
            .unwrap();
        assert_eq!(body.size(), 2);
        assert_eq!(body.get_item(&[1]).unwrap().size(), 0);
    }

    #[test]
    fn config_link_and_s2f35_body() {
        let cfg = DynamicEventReportConfig::new();
        let r = cfg.add_define_report(101, None, &[1]).unwrap();
        cfg.add_link_by_report(50, &[r]).unwrap();
        assert_eq!(cfg.link_count(), 1);
        let body = cfg.s2f35_body(Secs2::uint4([9]).unwrap()).unwrap();
        assert_eq!(u4_at(&body, &[0]), 9);
        assert_eq!(u4_at(&body, &[1, 0, 0]), 50); // CEID
        assert_eq!(u4_at(&body, &[1, 0, 1, 0]), 101); // RPTID
    }

    #[test]
    fn config_enable_ce_and_s2f37_body() {
        let cfg = DynamicEventReportConfig::new();
        cfg.add_enable_collection_event(Some("EV1".into()), 50)
            .unwrap();
        cfg.add_enable_collection_event(None, 51).unwrap();
        assert_eq!(cfg.event_count(), 2);
        assert!(cfg.get_collection_event_by_alias("EV1").is_some());

        let ceed = Ceed::Enable.secs2();
        let body = cfg.s2f37_enable_body(&ceed).unwrap();
        // L <BOOLEAN T> <L <U4 50> <U4 51>>
        assert_eq!(body.size(), 2);
        assert!(body.get_boolean_at(&[0, 0]).unwrap()); // CEED ENABLE = true
        assert_eq!(body.get_item(&[1]).unwrap().size(), 2);
        assert_eq!(u4_at(&body, &[1, 0]), 50);
        assert_eq!(u4_at(&body, &[1, 1]), 51);
    }
}
