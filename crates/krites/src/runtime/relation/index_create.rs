//! SessionTx methods for FTS, HNSW, and MinHash-LSH index creation.
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use compact_str::CompactString;
use pest::Parser;
use rmp_serde::Serializer;
use serde::Serialize;

use crate::data::relation::{ColType, ColumnDef, NullableColType, StoredRelationMetadata};
use crate::data::symb::Symbol;
use crate::data::tuple::TupleT;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fts::FtsIndexManifest;
use crate::parse::expr::build_expr;
use crate::parse::sys::{FtsIndexConfig, HnswIndexConfig, MinHashLshConfig};
use crate::parse::{DatalogParser, Rule};
use crate::runtime::error::{IndexAlreadyExistsSnafu, InvalidOperationSnafu, SerializationSnafu};
use crate::runtime::hnsw::HnswIndexManifest;
use crate::runtime::minhash_lsh::{HashPermutations, LshParams, MinHashLshIndexManifest, Weights};
use crate::runtime::transact::SessionTx;
use crate::utils::TempCollector;

use super::handles::{InputRelationHandle, RelationHandle, RelationId};

impl<'a> SessionTx<'a> {
    #[expect(
        clippy::expect_used,
        reason = "pest parse success guarantees at least one pair"
    )]
    pub(crate) fn create_minhash_lsh_index(&mut self, config: &MinHashLshConfig) -> Result<()> {
        let mut rel_handle = self.get_relation(&config.base_relation, true)?;

        if rel_handle.has_index(&config.index_name) {
            IndexAlreadyExistsSnafu {
                index_name: config.index_name.to_string(),
                relation_name: config.base_relation.to_string(),
            }
            .fail()?;
        }

        let inv_idx_keys = rel_handle.metadata.keys.clone();
        let inv_idx_vals = vec![ColumnDef {
            name: CompactString::from("minhash"),
            typing: NullableColType {
                coltype: ColType::Bytes,
                nullable: false,
            },
            default_gen: None,
        }];

        let mut idx_keys = vec![ColumnDef {
            name: CompactString::from("hash"),
            typing: NullableColType {
                coltype: ColType::Bytes,
                nullable: false,
            },
            default_gen: None,
        }];
        for k in rel_handle.metadata.keys.iter() {
            idx_keys.push(ColumnDef {
                name: format!("src_{}", k.name).into(),
                typing: k.typing.clone(),
                default_gen: None,
            });
        }
        let idx_vals = vec![];

        let idx_handle = self.write_idx_relation(
            &config.base_relation,
            &config.index_name,
            idx_keys,
            idx_vals,
        )?;

        let inv_idx_handle = self.write_idx_relation(
            &config.base_relation,
            &format!("{}:inv", config.index_name),
            inv_idx_keys,
            inv_idx_vals,
        )?;

        let params = LshParams::find_optimal_params(
            config.target_threshold.0,
            config.n_perm,
            &Weights(
                config.false_positive_weight.0,
                config.false_negative_weight.0,
            ),
        );
        let num_perm = params.b * params.r;
        let perms = HashPermutations::new(num_perm);
        let manifest = MinHashLshIndexManifest {
            base_relation: config.base_relation.clone(),
            index_name: config.index_name.clone(),
            extractor: config.extractor.clone(),
            n_gram: config.n_gram,
            tokenizer: config.tokenizer.clone(),
            filters: config.filters.clone(),
            num_perm,
            n_bands: params.b,
            n_rows_in_band: params.r,
            threshold: config.target_threshold.0,
            perms: perms.as_bytes().to_vec(),
        };

        let tokenizer =
            self.tokenizers
                .get(&idx_handle.name, &manifest.tokenizer, &manifest.filters)?;
        let parsed = DatalogParser::parse(Rule::expr, &manifest.extractor)
            .map_err(|e| crate::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "index",
                    reason: e.to_string(),
                }
                .build(),
            })?
            .next()
            .expect("pest parse succeeded but produced no pairs");
        let mut code_expr = build_expr(parsed, &Default::default())?;
        let binding_map = rel_handle.raw_binding_map();
        code_expr.fill_binding_indices(&binding_map)?;
        let extractor = code_expr.compile()?;

        let mut stack = vec![];

        let hash_perms = manifest.get_hash_perms()?;
        let mut existing = TempCollector::default();
        for tuple in rel_handle.scan_all(self) {
            existing.push(tuple?);
        }

        for tuple in existing.into_iter() {
            self.put_lsh_index_item(
                &tuple,
                &extractor,
                &mut stack,
                &tokenizer,
                &rel_handle,
                &idx_handle,
                &inv_idx_handle,
                &manifest,
                &hash_perms,
            )?;
        }

        rel_handle.lsh_indices.insert(
            manifest.index_name.clone(),
            (idx_handle, inv_idx_handle, manifest),
        );

        let new_encoded =
            vec![DataValue::from(&rel_handle.name as &str)].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        rel_handle
            .serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&new_encoded, &meta_val)?;

        Ok(())
    }

    #[expect(
        clippy::expect_used,
        reason = "pest parse success guarantees at least one pair"
    )]
    pub(crate) fn create_fts_index(&mut self, config: &FtsIndexConfig) -> Result<()> {
        let mut rel_handle = self.get_relation(&config.base_relation, true)?;

        if rel_handle.has_index(&config.index_name) {
            IndexAlreadyExistsSnafu {
                index_name: config.index_name.to_string(),
                relation_name: config.base_relation.to_string(),
            }
            .fail()?;
        }

        let mut idx_keys: Vec<ColumnDef> = vec![ColumnDef {
            name: CompactString::from("word"),
            typing: NullableColType {
                coltype: ColType::String,
                nullable: false,
            },
            default_gen: None,
        }];

        for k in rel_handle.metadata.keys.iter() {
            idx_keys.push(ColumnDef {
                name: format!("src_{}", k.name).into(),
                typing: k.typing.clone(),
                default_gen: None,
            });
        }

        let col_type = NullableColType {
            coltype: ColType::List {
                eltype: Box::new(NullableColType {
                    coltype: ColType::Int,
                    nullable: false,
                }),
                len: None,
            },
            nullable: false,
        };

        let non_idx_keys: Vec<ColumnDef> = vec![
            ColumnDef {
                name: CompactString::from("offset_from"),
                typing: col_type.clone(),
                default_gen: None,
            },
            ColumnDef {
                name: CompactString::from("offset_to"),
                typing: col_type.clone(),
                default_gen: None,
            },
            ColumnDef {
                name: CompactString::from("position"),
                typing: col_type,
                default_gen: None,
            },
            ColumnDef {
                name: CompactString::from("total_length"),
                typing: NullableColType {
                    coltype: ColType::Int,
                    nullable: false,
                },
                default_gen: None,
            },
        ];

        let idx_handle = self.write_idx_relation(
            &config.base_relation,
            &config.index_name,
            idx_keys,
            non_idx_keys,
        )?;

        let manifest = FtsIndexManifest {
            base_relation: config.base_relation.clone(),
            index_name: config.index_name.clone(),
            extractor: config.extractor.clone(),
            tokenizer: config.tokenizer.clone(),
            filters: config.filters.clone(),
        };

        let tokenizer =
            self.tokenizers
                .get(&idx_handle.name, &manifest.tokenizer, &manifest.filters)?;

        let parsed = DatalogParser::parse(Rule::expr, &manifest.extractor)
            .map_err(|e| crate::error::InternalError::Runtime {
                source: InvalidOperationSnafu {
                    op: "index",
                    reason: e.to_string(),
                }
                .build(),
            })?
            .next()
            .expect("pest parse succeeded but produced no pairs");
        let mut code_expr = build_expr(parsed, &Default::default())?;
        let binding_map = rel_handle.raw_binding_map();
        code_expr.fill_binding_indices(&binding_map)?;
        let extractor = code_expr.compile()?;

        let mut stack = vec![];

        let mut existing = TempCollector::default();
        for tuple in rel_handle.scan_all(self) {
            existing.push(tuple?);
        }
        for tuple in existing.into_iter() {
            let key_part = &tuple[..rel_handle.metadata.keys.len()];
            if rel_handle.exists(self, key_part)? {
                self.del_fts_index_item(
                    &tuple,
                    &extractor,
                    &mut stack,
                    &tokenizer,
                    &rel_handle,
                    &idx_handle,
                )?;
            }
            self.put_fts_index_item(
                &tuple,
                &extractor,
                &mut stack,
                &tokenizer,
                &rel_handle,
                &idx_handle,
            )?;
        }

        rel_handle
            .fts_indices
            .insert(manifest.index_name.clone(), (idx_handle, manifest));

        let new_encoded =
            vec![DataValue::from(&rel_handle.name as &str)].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        rel_handle
            .serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&new_encoded, &meta_val)?;

        Ok(())
    }

    #[expect(
        clippy::expect_used,
        reason = "pest parse success guarantees at least one pair"
    )]
    pub(crate) fn create_hnsw_index(&mut self, config: &HnswIndexConfig) -> Result<()> {
        let mut rel_handle = self.get_relation(&config.base_relation, true)?;

        if rel_handle.has_index(&config.index_name) {
            IndexAlreadyExistsSnafu {
                index_name: config.index_name.to_string(),
                relation_name: config.base_relation.to_string(),
            }
            .fail()?;
        }

        if config.vec_fields.is_empty() {
            InvalidOperationSnafu {
                op: "create HNSW index",
                reason: "no vector fields specified",
            }
            .fail()?;
        }
        let mut vec_field_indices = vec![];
        for field in config.vec_fields.iter() {
            let mut found = false;
            for (i, col) in rel_handle
                .metadata
                .keys
                .iter()
                .chain(rel_handle.metadata.non_keys.iter())
                .enumerate()
            {
                if col.name == *field {
                    let mut col_type = col.typing.coltype.clone();
                    if let ColType::List { eltype, .. } = &col_type {
                        col_type = eltype.coltype.clone();
                    }

                    if let ColType::Vec { eltype, len } = col_type {
                        if eltype != config.dtype {
                            InvalidOperationSnafu {
                                op: "create HNSW index",
                                reason: format!(
                                    "field {field} has type {eltype:?} (expected {:?})",
                                    config.dtype
                                ),
                            }
                            .fail()?;
                        }
                        if len != config.vec_dim {
                            InvalidOperationSnafu {
                                op: "create HNSW index",
                                reason: format!(
                                    "field {field} has dimension {len} (expected {})",
                                    config.vec_dim
                                ),
                            }
                            .fail()?;
                        }
                    } else {
                        InvalidOperationSnafu {
                            op: "create HNSW index",
                            reason: format!("field {field} is not a vector type"),
                        }
                        .fail()?;
                    }

                    found = true;
                    vec_field_indices.push(i);
                    break;
                }
            }
            if !found {
                InvalidOperationSnafu {
                    op: "create HNSW index",
                    reason: format!("field {field} does not exist"),
                }
                .fail()?;
            }
        }

        let mut idx_keys: Vec<ColumnDef> = vec![ColumnDef {
            name: CompactString::from("layer"),
            typing: NullableColType {
                coltype: ColType::Int,
                nullable: false,
            },
            default_gen: None,
        }];
        for prefix in ["fr", "to"] {
            for col in rel_handle.metadata.keys.iter() {
                let mut col = col.clone();
                col.name = CompactString::from(format!("{}_{}", prefix, col.name));
                idx_keys.push(col);
            }
            idx_keys.push(ColumnDef {
                name: CompactString::from(format!("{}__field", prefix)),
                typing: NullableColType {
                    coltype: ColType::Int,
                    nullable: false,
                },
                default_gen: None,
            });
            idx_keys.push(ColumnDef {
                name: CompactString::from(format!("{}__sub_idx", prefix)),
                typing: NullableColType {
                    coltype: ColType::Int,
                    nullable: false,
                },
                default_gen: None,
            });
        }

        let non_idx_keys = vec![
            ColumnDef {
                name: CompactString::from("dist"),
                typing: NullableColType {
                    coltype: ColType::Float,
                    nullable: false,
                },
                default_gen: None,
            },
            ColumnDef {
                name: CompactString::from("hash"),
                typing: NullableColType {
                    coltype: ColType::Bytes,
                    nullable: true,
                },
                default_gen: None,
            },
            ColumnDef {
                name: CompactString::from("ignore_link"),
                typing: NullableColType {
                    coltype: ColType::Bool,
                    nullable: false,
                },
                default_gen: None,
            },
        ];
        let idx_handle = self.write_idx_relation(
            &config.base_relation,
            &config.index_name,
            idx_keys,
            non_idx_keys,
        )?;

        let manifest = HnswIndexManifest {
            base_relation: config.base_relation.clone(),
            index_name: config.index_name.clone(),
            vec_dim: config.vec_dim,
            dtype: config.dtype,
            vec_fields: vec_field_indices,
            distance: config.distance,
            ef_construction: config.ef_construction,
            m_neighbours: config.m_neighbours,
            m_max: config.m_neighbours,
            m_max0: config.m_neighbours * 2,
            level_multiplier: 1. / (config.m_neighbours as f64).ln(),
            index_filter: config.index_filter.clone(),
            extend_candidates: config.extend_candidates,
            keep_pruned_connections: config.keep_pruned_connections,
            max_vectors: None,
        };

        let mut all_tuples = TempCollector::default();
        for tuple in rel_handle.scan_all(self) {
            all_tuples.push(tuple?);
        }
        let filter = if let Some(f_code) = &manifest.index_filter {
            let parsed = DatalogParser::parse(Rule::expr, f_code)
                .map_err(|e| crate::error::InternalError::Runtime {
                    source: InvalidOperationSnafu {
                        op: "index",
                        reason: e.to_string(),
                    }
                    .build(),
                })?
                .next()
                .expect("pest parse succeeded but produced no pairs");
            let mut code_expr = build_expr(parsed, &Default::default())?;
            let binding_map = rel_handle.raw_binding_map();
            code_expr.fill_binding_indices(&binding_map)?;
            code_expr.compile()?
        } else {
            vec![]
        };
        let filter = if filter.is_empty() {
            None
        } else {
            Some(&filter)
        };
        let mut stack = vec![];
        for tuple in all_tuples.into_iter() {
            self.hnsw_put(
                &manifest,
                &rel_handle,
                &idx_handle,
                filter,
                &mut stack,
                &tuple,
            )?;
        }

        rel_handle
            .hnsw_indices
            .insert(config.index_name.clone(), (idx_handle, manifest));

        let new_encoded =
            vec![DataValue::from(&config.base_relation as &str)].encode_as_key(RelationId::SYSTEM);
        let mut meta_val = vec![];
        rel_handle
            .serialize(&mut Serializer::new(&mut meta_val))
            .map_err(|e| {
                SerializationSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        self.store_tx.put(&new_encoded, &meta_val)?;

        Ok(())
    }

    fn write_idx_relation(
        &mut self,
        base_name: &str,
        idx_name: &str,
        idx_keys: Vec<ColumnDef>,
        non_idx_keys: Vec<ColumnDef>,
    ) -> Result<RelationHandle> {
        let key_bindings = idx_keys
            .iter()
            .map(|col| Symbol::new(col.name.clone(), Default::default()))
            .collect();
        let dep_bindings = non_idx_keys
            .iter()
            .map(|col| Symbol::new(col.name.clone(), Default::default()))
            .collect();
        let idx_handle = InputRelationHandle {
            name: Symbol::new(format!("{}:{}", base_name, idx_name), Default::default()),
            metadata: StoredRelationMetadata {
                keys: idx_keys,
                non_keys: non_idx_keys,
            },
            key_bindings,
            dep_bindings,
            span: Default::default(),
        };
        let idx_handle = self.create_relation(idx_handle)?;
        Ok(idx_handle)
    }
}
