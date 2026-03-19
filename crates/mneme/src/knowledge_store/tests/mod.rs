#[cfg(all(test, feature = "mneme-engine"))]
mod engine_assertions {
    use super::super::KnowledgeStore;
    use static_assertions::assert_impl_all;
    assert_impl_all!(KnowledgeStore: Send, Sync);
}

#[cfg(feature = "mneme-engine")]
mod causal;
mod ddl;
#[cfg(feature = "mneme-engine")]
mod entities;
#[cfg(feature = "mneme-engine")]
mod facts;
#[cfg(feature = "mneme-engine")]
mod lesson_e2e;
#[cfg(feature = "mneme-engine")]
mod proptests;
#[cfg(feature = "mneme-engine")]
mod search;
#[cfg(feature = "mneme-engine")]
mod skills;
#[cfg(feature = "mneme-engine")]
mod temporal;
