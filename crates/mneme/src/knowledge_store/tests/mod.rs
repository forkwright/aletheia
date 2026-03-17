#[cfg(all(test, feature = "mneme-engine"))]
mod engine_assertions {
    use super::super::KnowledgeStore;
    use static_assertions::assert_impl_all;
    assert_impl_all!(KnowledgeStore: Send, Sync);
}

mod ddl;
#[cfg(feature = "mneme-engine")]
mod entities;
#[cfg(feature = "mneme-engine")]
mod facts;
#[cfg(feature = "mneme-engine")]
mod proptests;
#[cfg(feature = "mneme-engine")]
mod search;
#[cfg(feature = "mneme-engine")]
mod skills;
#[cfg(feature = "mneme-engine")]
mod temporal;
