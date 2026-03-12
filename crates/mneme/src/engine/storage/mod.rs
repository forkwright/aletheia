//! Storage backend abstraction layer.
pub(crate) mod error;

use error::StorageResult;
use itertools::Itertools;

use crate::engine::data::tuple::Tuple;
use crate::engine::data::value::ValidityTs;
use crate::engine::error::InternalResult;
use crate::engine::runtime::relation::decode_tuple_from_kv;

#[cfg(feature = "storage-fjall")]
pub(crate) mod fjall_backend;
pub(crate) mod mem;
pub(crate) mod temp;

/// Swappable storage trait for the mneme engine.
pub trait Storage<'s>: Send + Sync + Clone {
    /// The associated transaction type used by this engine
    type Tx: StoreTx<'s>;

    /// Returns a string that identifies the storage kind
    fn storage_kind(&self) -> &'static str;

    /// Create a transaction object. Write ops will only be called when `write == true`.
    fn transact(&'s self, write: bool) -> StorageResult<Self::Tx>;

    /// Compact the key range. Can be a no-op if the storage engine does not
    /// have the concept of compaction.
    fn range_compact(&'s self, lower: &[u8], upper: &[u8]) -> StorageResult<()>;

    /// Put multiple key-value pairs into the database.
    /// No duplicate data will be sent, and the order data come in is strictly ascending.
    /// There will be no other access to the database while this function is running.
    fn batch_put<'a>(
        &'a self,
        data: Box<dyn Iterator<Item = StorageResult<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> StorageResult<()>;
}

/// Trait for the associated transaction type of a storage engine.
/// A transaction needs to guarantee MVCC semantics for all operations.
pub trait StoreTx<'s>: Sync {
    /// Get a key. If `for_update` is `true` (only possible in a write transaction),
    /// then the database needs to guarantee that `commit()` can only succeed if
    /// the key has not been modified outside the transaction.
    fn get(&self, key: &[u8], for_update: bool) -> StorageResult<Option<Vec<u8>>>;

    /// Put a key-value pair into the storage. In case of existing key,
    /// the storage engine needs to overwrite the old value.
    fn put(&mut self, key: &[u8], val: &[u8]) -> StorageResult<()>;

    /// Should return true if the engine supports parallel put, false otherwise.
    fn supports_par_put(&self) -> bool;

    /// Put a key-value pair into the storage. In case of existing key,
    /// the storage engine needs to overwrite the old value.
    /// The difference between this one and `put` is the mutability of self.
    /// It is OK to always panic if `supports_par_put` returns `false`.
    fn par_put(&self, _key: &[u8], _val: &[u8]) -> StorageResult<()> {
        panic!("par_put is not supported")
    }

    /// Delete a key-value pair from the storage.
    fn del(&mut self, key: &[u8]) -> StorageResult<()>;

    /// Delete a range from persisted data only.
    fn del_range_from_persisted(&mut self, lower: &[u8], upper: &[u8]) -> StorageResult<()>;

    /// Check if a key exists. If `for_update` is `true` (only possible in a write transaction),
    /// then the database needs to guarantee that `commit()` can only succeed if
    /// the key has not been modified outside the transaction.
    fn exists(&self, key: &[u8], for_update: bool) -> StorageResult<bool>;

    /// Commit a transaction. Must return an `Err` if MVCC consistency cannot be guaranteed,
    /// and discard all changes introduced by this transaction.
    fn commit(&mut self) -> StorageResult<()>;

    /// Scan on a range. `lower` is inclusive whereas `upper` is exclusive.
    /// The default implementation calls [`range_scan`](Self::range_scan) and converts the results.
    ///
    /// The implementation must call [`decode_tuple_from_kv`](crate::engine::decode_tuple_from_kv) to obtain
    /// a decoded tuple in the loop of the iterator.
    ///
    /// Iterator items use [`InternalResult`] for compatibility with engine-internal callers.
    fn range_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>
    where
        's: 'a,
    {
        let it = self.range_scan(lower, upper);
        Box::new(it.map_ok(|(k, v)| decode_tuple_from_kv(&k, &v, None)))
    }

    /// Scan on a range with a certain validity.
    ///
    /// `lower` is inclusive whereas `upper` is exclusive.
    /// For tuples that differ only with respect to their validity, which must be at
    /// the last slot of the key,
    /// only the tuple that has validity equal to or earlier than (i.e. greater by the comparator)
    /// `valid_at` should be considered for returning, and only those with an assertive validity
    /// should be returned. Every other tuple should be skipped.
    ///
    /// Ideally, implementations should take advantage of seeking capabilities of the
    /// underlying storage so that not every tuple within the `lower` and `upper` range
    /// need to be looked at.
    ///
    /// For custom implementations, it is OK to return an iterator that always error out,
    /// in which case the database with the engine does not support time travelling.
    /// You should indicate this clearly in your error message.
    ///
    /// Iterator items use [`InternalResult`] for compatibility with engine-internal callers.
    fn range_skip_scan_tuple<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>;

    /// Scan on a range and return the raw results.
    /// `lower` is inclusive whereas `upper` is exclusive.
    ///
    /// Iterator items use [`InternalResult`] for compatibility with engine-internal callers.
    fn range_scan<'a>(
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a>
    where
        's: 'a;

    /// Return the number of rows in the range.
    fn range_count<'a>(&'a self, lower: &[u8], upper: &[u8]) -> StorageResult<usize>
    where
        's: 'a;
}
