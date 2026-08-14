//! Dynamic event-report models (Define-Report / Link / CEID).
//!
//! Source: `DynamicReport` / `DynamicLink` / `DynamicCollectionEvent`.
//! Immutable value types; registry/body assembly in [`super::dynamic_config`].

use crate::secs2::{self, Secs2};

/// Define-Report entry (RPTID + VIDs). Immutable.
///
/// `ToS2F33Report` → `L <RPTID> <L <VID…>>`.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicReport {
    report_id: Secs2,
    alias: Option<String>,
    vids: Vec<Secs2>,
}

impl DynamicReport {
    /// `DynamicReport.NewInstance(reportId, alias, vids)`.
    pub fn new(
        report_id: Secs2,
        alias: Option<String>,
        vids: impl IntoIterator<Item = Secs2>,
    ) -> Self {
        Self {
            report_id,
            alias,
            vids: vids.into_iter().collect(),
        }
    }

    pub fn report_id(&self) -> &Secs2 {
        &self.report_id
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    pub fn vids(&self) -> &[Secs2] {
        &self.vids
    }

    /// `ToS2F33Report()` → `L <RPTID> <L VIDs>`.
    pub fn to_s2f33_report(&self) -> secs2::Result<Secs2> {
        let vids = Secs2::list(self.vids.iter().cloned())?;
        Secs2::list([self.report_id.clone(), vids])
    }

    /// `FromS2F33Report` — alias cleared (source always null).
    pub fn from_s2f33_report(secs2: &Secs2) -> secs2::Result<Self> {
        let report_id = secs2.get_item(&[0])?.clone();
        let vids_item = secs2.get_item(&[1])?;
        let vids = list_children(vids_item)?;
        Ok(Self::new(report_id, None, vids))
    }
}

/// Collection-Event (CEID) for enable/disable (S2F37). Immutable.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicCollectionEvent {
    alias: Option<String>,
    collection_event_id: Secs2,
}

impl DynamicCollectionEvent {
    /// `DynamicCollectionEvent.NewInstance(alias, ceid)`.
    pub fn new(alias: Option<String>, collection_event_id: Secs2) -> Self {
        Self {
            alias,
            collection_event_id,
        }
    }

    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    pub fn collection_event_id(&self) -> &Secs2 {
        &self.collection_event_id
    }

    /// `ToS2F37CollectionEvent()` — the CEID item itself.
    pub fn to_s2f37_collection_event(&self) -> &Secs2 {
        &self.collection_event_id
    }

    /// `FromS2F37CollectionEvent`.
    pub fn from_s2f37_collection_event(secs2: Secs2) -> Self {
        Self::new(None, secs2)
    }
}

/// Event–Report link (CEID + RPTIDs). Immutable.
///
/// `ToS2F35Link` → `L <CEID> <L <RPTID…>>`.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicLink {
    collection_event: DynamicCollectionEvent,
    report_ids: Vec<Secs2>,
}

impl DynamicLink {
    /// `DynamicLink.NewInstance(ce, reportIds)`.
    pub fn new(
        collection_event: DynamicCollectionEvent,
        report_ids: impl IntoIterator<Item = Secs2>,
    ) -> Self {
        Self {
            collection_event,
            report_ids: report_ids.into_iter().collect(),
        }
    }

    pub fn collection_event(&self) -> &DynamicCollectionEvent {
        &self.collection_event
    }

    pub fn collection_event_id(&self) -> &Secs2 {
        self.collection_event.collection_event_id()
    }

    pub fn report_ids(&self) -> &[Secs2] {
        &self.report_ids
    }

    /// `ToS2F35Link()`.
    pub fn to_s2f35_link(&self) -> secs2::Result<Secs2> {
        let rpts = Secs2::list(self.report_ids.iter().cloned())?;
        Secs2::list([self.collection_event_id().clone(), rpts])
    }

    /// `FromS2F35Link`.
    pub fn from_s2f35_link(secs2: &Secs2) -> secs2::Result<Self> {
        let ceid = secs2.get_item(&[0])?.clone();
        let rpts_item = secs2.get_item(&[1])?;
        let report_ids = list_children(rpts_item)?;
        let ce = DynamicCollectionEvent::new(None, ceid);
        Ok(Self::new(ce, report_ids))
    }
}

fn list_children(item: &Secs2) -> secs2::Result<Vec<Secs2>> {
    match item {
        Secs2::List(v) => Ok(v.clone()),
        _ => Err(secs2::Error::IllegalDataFormat("Not Secs2List")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// U4 single-value leaf: path ends with element index 0.
    fn u4_val(s: &Secs2, path: &[usize]) -> i64 {
        let mut full = path.to_vec();
        full.push(0);
        s.get_long_at(&full).unwrap()
    }

    #[test]
    fn dynamic_report_s2f33_roundtrip() {
        let rptid = Secs2::uint4([101]).unwrap();
        let vids = vec![
            Secs2::uint4([1]).unwrap(),
            Secs2::uint4([2]).unwrap(),
            Secs2::uint4([3]).unwrap(),
        ];
        let r = DynamicReport::new(rptid, Some("R1".into()), vids);
        assert_eq!(r.alias(), Some("R1"));
        assert_eq!(r.vids().len(), 3);

        let s2 = r.to_s2f33_report().unwrap();
        // L <U4 101> <L <U4 1> <U4 2> <U4 3>>
        assert_eq!(s2.size(), 2);
        assert_eq!(u4_val(&s2, &[0]), 101);
        assert_eq!(u4_val(&s2, &[1, 0]), 1);
        assert_eq!(u4_val(&s2, &[1, 1]), 2);
        assert_eq!(u4_val(&s2, &[1, 2]), 3);

        let r2 = DynamicReport::from_s2f33_report(&s2).unwrap();
        assert!(r2.alias().is_none()); // FromS2F33 clears alias
        assert_eq!(r2.vids().len(), 3);
        assert_eq!(u4_val(r2.report_id(), &[]), 101);
        assert_eq!(u4_val(&r2.vids()[1], &[]), 2);
    }

    #[test]
    fn dynamic_link_s2f35_roundtrip() {
        let ce = DynamicCollectionEvent::new(Some("E1".into()), Secs2::uint4([50]).unwrap());
        assert_eq!(ce.alias(), Some("E1"));
        assert_eq!(u4_val(ce.to_s2f37_collection_event(), &[]), 50);

        let link = DynamicLink::new(
            ce,
            [
                Secs2::uint4([101]).unwrap(),
                Secs2::uint4([102]).unwrap(),
            ],
        );
        let s2 = link.to_s2f35_link().unwrap();
        assert_eq!(u4_val(&s2, &[0]), 50);
        assert_eq!(u4_val(&s2, &[1, 0]), 101);
        assert_eq!(u4_val(&s2, &[1, 1]), 102);

        let link2 = DynamicLink::from_s2f35_link(&s2).unwrap();
        assert_eq!(u4_val(link2.collection_event_id(), &[]), 50);
        assert_eq!(link2.report_ids().len(), 2);
        assert_eq!(u4_val(&link2.report_ids()[1], &[]), 102);
    }

    #[test]
    fn dynamic_collection_event_s2f37() {
        let ceid = Secs2::uint4([9]).unwrap();
        let ce = DynamicCollectionEvent::from_s2f37_collection_event(ceid.clone());
        assert!(ce.alias().is_none());
        assert_eq!(ce.to_s2f37_collection_event(), &ceid);
        assert_eq!(u4_val(ce.collection_event_id(), &[]), 9);
    }
}
