use jiff::Timestamp;

use super::*;

fn ts() -> Timestamp {
    Timestamp::UNIX_EPOCH
}

fn manual_fact(id: &str, value: Scalar, unit: Unit) -> Fact {
    Fact {
        id: FactId::new(id).unwrap(),
        value,
        unit,
        source: Source::Manual {
            note: "test".to_owned(),
            captured_by: "tester".to_owned(),
        },
        captured: ts(),
    }
}

#[test]
fn empty_factbase_validates() {
    let fb = Factbase::new();
    assert!(fb.validate().is_ok());
    let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
    assert!(resolved.is_empty());
}

#[test]
fn manual_facts_resolve_to_authored_value() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 7 }, Unit::Count));
    let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
    let a = resolved.get(&FactId::new("a").unwrap()).unwrap();
    assert_eq!(a.value, Scalar::Count { value: 7 });
}

#[test]
fn derived_sum_resolves() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 3 }, Unit::Count));
    fb.add_fact(manual_fact("b", Scalar::Count { value: 4 }, Unit::Count));
    fb.add_fact(Fact {
        id: FactId::new("total").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("a").unwrap(),
                b: FactId::new("b").unwrap(),
            },
            inputs: vec![FactId::new("a").unwrap(), FactId::new("b").unwrap()],
        },
        captured: ts(),
    });
    let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
    let total = resolved.get(&FactId::new("total").unwrap()).unwrap();
    assert_eq!(total.value, Scalar::Count { value: 7 });
}

#[test]
fn reference_resolves_to_target_value() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact(
        "source",
        Scalar::Count { value: 42 },
        Unit::Count,
    ));
    fb.add_fact(Fact {
        id: FactId::new("alias").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("source").unwrap(),
        },
        captured: ts(),
    });
    let resolved = fb.resolve(&DataSourceRegistry::new()).unwrap();
    let alias = resolved.get(&FactId::new("alias").unwrap()).unwrap();
    assert_eq!(alias.value, Scalar::Count { value: 42 });
}

#[test]
fn cycle_is_detected_with_path_in_error() {
    let mut fb = Factbase::new();
    fb.add_fact(Fact {
        id: FactId::new("a").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("b").unwrap(),
        },
        captured: ts(),
    });
    fb.add_fact(Fact {
        id: FactId::new("b").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("a").unwrap(),
        },
        captured: ts(),
    });
    let err = fb.validate().expect_err("cycle must be detected");
    let path = match err {
        FactbaseError::Cycle { path } => path,
        other => panic!("expected Cycle, got {other:?}"),
    };
    assert!(path.contains(&"a".to_owned()));
    assert!(path.contains(&"b".to_owned()));
}

#[test]
fn unknown_reference_rejects_with_named_id() {
    let mut fb = Factbase::new();
    fb.add_fact(Fact {
        id: FactId::new("orphan").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Reference {
            fact: FactId::new("missing").unwrap(),
        },
        captured: ts(),
    });
    let err = fb.validate().expect_err("dangling reference must reject");
    assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "missing"));
}

#[test]
fn unknown_claim_target_rejects() {
    let mut fb = Factbase::new();
    fb.add_claim(Claim {
        id: ClaimId::new("c1").unwrap(),
        text: "x is 1".to_owned(),
        asserts: FactId::new("absent").unwrap(),
        location: Location {
            at: "deck/slide/1".to_owned(),
        },
        tolerance: Tolerance::STRICT,
    });
    let err = fb.validate().expect_err("claim of absent fact rejects");
    assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "absent"));
}

#[test]
fn sql_without_adapter_rejects_with_named_data_source() {
    let mut fb = Factbase::new();
    fb.add_fact(Fact {
        id: FactId::new("from_db").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Sql {
            data_source: DataSourceId::new("redshift_prod").unwrap(),
            query: "SELECT 1".to_owned(),
            table: "totals".to_owned(),
        },
        captured: ts(),
    });
    let err = fb
        .resolve(&DataSourceRegistry::new())
        .expect_err("missing adapter");
    match err {
        FactbaseError::MissingDataSource { data_source, .. } => {
            assert_eq!(data_source, "redshift_prod");
        }
        other => panic!("expected MissingDataSource, got {other:?}"),
    }
}

#[test]
fn type_mismatch_in_derived_rejects() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact(
        "count",
        Scalar::Count { value: 3 },
        Unit::Count,
    ));
    fb.add_fact(manual_fact(
        "money",
        Scalar::Money {
            value: Money::from_units(7).expect("in range"),
        },
        Unit::Usd,
    ));
    fb.add_fact(Fact {
        id: FactId::new("mix").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("count").unwrap(),
                b: FactId::new("money").unwrap(),
            },
            inputs: vec![FactId::new("count").unwrap(), FactId::new("money").unwrap()],
        },
        captured: ts(),
    });
    let err = fb
        .resolve(&DataSourceRegistry::new())
        .expect_err("type mismatch");
    assert!(matches!(err, FactbaseError::DerivedTypeMismatch { .. }));
}

#[test]
fn walk_chain_leaf_fact() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    let chain = fb.walk_citation_chain(&FactId::new("a").unwrap());
    assert!(chain.is_empty());
}

#[test]
fn walk_chain_derived_returns_leaves_first() {
    let mut fb = Factbase::new();
    let id_a = FactId::new("a").unwrap();
    let id_b = FactId::new("b").unwrap();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    fb.add_fact(Fact {
        id: id_b.clone(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: id_a.clone(),
                b: id_a.clone(),
            },
            inputs: vec![id_a.clone()],
        },
        captured: ts(),
    });
    let chain = fb.walk_citation_chain(&id_b);
    assert_eq!(chain, vec![id_a]);
}

#[test]
fn walk_chain_diamond() {
    let mut fb = Factbase::new();
    let id_a = FactId::new("a").unwrap();
    let id_b = FactId::new("b").unwrap();
    let id_c = FactId::new("c").unwrap();
    let id_d = FactId::new("d").unwrap();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    fb.add_fact(Fact {
        id: id_b.clone(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: id_a.clone(),
                b: id_a.clone(),
            },
            inputs: vec![id_a.clone()],
        },
        captured: ts(),
    });
    fb.add_fact(Fact {
        id: id_c.clone(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: id_a.clone(),
                b: id_a.clone(),
            },
            inputs: vec![id_a.clone()],
        },
        captured: ts(),
    });
    fb.add_fact(Fact {
        id: id_d.clone(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: id_b.clone(),
                b: id_c.clone(),
            },
            inputs: vec![id_b.clone(), id_c.clone()],
        },
        captured: ts(),
    });
    let chain = fb.walk_citation_chain(&id_d);
    assert_eq!(chain, vec![id_a, id_b, id_c]);
}

#[test]
fn walk_chain_unknown_root() {
    let fb = Factbase::new();
    let chain = fb.walk_citation_chain(&FactId::new("ghost").unwrap());
    assert!(chain.is_empty());
}

#[test]
fn claim_citation_chain_returns_fact_chain() {
    let mut fb = Factbase::new();
    let id_a = FactId::new("a").unwrap();
    let id_b = FactId::new("b").unwrap();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    fb.add_fact(Fact {
        id: id_b.clone(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: id_a.clone(),
                b: id_a.clone(),
            },
            inputs: vec![id_a.clone()],
        },
        captured: ts(),
    });
    fb.add_claim(Claim {
        id: ClaimId::new("c1").unwrap(),
        text: "b is 1".to_owned(),
        asserts: id_b.clone(),
        location: Location {
            at: "deck/slide/1".to_owned(),
        },
        tolerance: Tolerance::STRICT,
    });
    let chain = fb.claim_citation_chain(&ClaimId::new("c1").unwrap());
    assert_eq!(chain, Some(vec![id_a]));
}

#[test]
fn claim_citation_chain_missing_claim() {
    let fb = Factbase::new();
    let chain = fb.claim_citation_chain(&ClaimId::new("ghost").unwrap());
    assert_eq!(chain, None);
}

#[test]
fn derived_formula_unknown_fact_rejects_even_when_inputs_valid() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    fb.add_fact(Fact {
        id: FactId::new("d").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("a").unwrap(),
                b: FactId::new("b").unwrap(),
            },
            inputs: vec![FactId::new("a").unwrap()],
        },
        captured: ts(),
    });
    let err = fb
        .validate()
        .expect_err("formula ref outside inputs must reject");
    assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "b"));
}

#[test]
fn money_add_overflow_rejects() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact(
        "a",
        Scalar::Money {
            value: Money::from_micros(i64::MAX),
        },
        Unit::Usd,
    ));
    fb.add_fact(manual_fact(
        "b",
        Scalar::Money {
            value: Money::from_micros(1),
        },
        Unit::Usd,
    ));
    fb.add_fact(Fact {
        id: FactId::new("sum").unwrap(),
        value: Scalar::Money {
            value: Money::from_micros(0),
        },
        unit: Unit::Usd,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("a").unwrap(),
                b: FactId::new("b").unwrap(),
            },
            inputs: vec![FactId::new("a").unwrap(), FactId::new("b").unwrap()],
        },
        captured: ts(),
    });
    let err = fb
        .resolve(&DataSourceRegistry::new())
        .expect_err("money add overflow must reject");
    assert!(matches!(err, FactbaseError::BadDerived { .. }));
}

#[test]
fn money_sub_overflow_rejects() {
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact(
        "a",
        Scalar::Money {
            value: Money::from_micros(i64::MIN),
        },
        Unit::Usd,
    ));
    fb.add_fact(manual_fact(
        "b",
        Scalar::Money {
            value: Money::from_micros(1),
        },
        Unit::Usd,
    ));
    fb.add_fact(Fact {
        id: FactId::new("diff").unwrap(),
        value: Scalar::Money {
            value: Money::from_micros(0),
        },
        unit: Unit::Usd,
        source: Source::Derived {
            formula: Expr::Sub {
                a: FactId::new("a").unwrap(),
                b: FactId::new("b").unwrap(),
            },
            inputs: vec![FactId::new("a").unwrap(), FactId::new("b").unwrap()],
        },
        captured: ts(),
    });
    let err = fb
        .resolve(&DataSourceRegistry::new())
        .expect_err("money sub overflow must reject");
    assert!(matches!(err, FactbaseError::BadDerived { .. }));
}

#[test]
fn derived_inputs_formula_divergence_rejects_with_distinct_variants() {
    // Case 1: formula references a fact that exists in the factbase but is
    // omitted from the derived fact's `inputs`.
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    fb.add_fact(manual_fact("b", Scalar::Count { value: 2 }, Unit::Count));
    fb.add_fact(Fact {
        id: FactId::new("d1").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("a").unwrap(),
                b: FactId::new("b").unwrap(),
            },
            inputs: vec![FactId::new("a").unwrap()],
        },
        captured: ts(),
    });
    let err = fb
        .validate()
        .expect_err("formula ref in factbase but outside inputs must reject");
    assert!(matches!(
        err,
        FactbaseError::FactInputsMissing { id, derived_fact }
            if id == "b" && derived_fact == "d1"
    ));

    // Case 2: formula references a fact that does not exist in the factbase at
    // all. This must keep returning `UnknownFact`, distinct from the missing-
    // from-inputs variant.
    let mut fb = Factbase::new();
    fb.add_fact(manual_fact("a", Scalar::Count { value: 1 }, Unit::Count));
    fb.add_fact(Fact {
        id: FactId::new("d2").unwrap(),
        value: Scalar::Count { value: 0 },
        unit: Unit::Count,
        source: Source::Derived {
            formula: Expr::Add {
                a: FactId::new("a").unwrap(),
                b: FactId::new("z").unwrap(),
            },
            inputs: vec![FactId::new("a").unwrap()],
        },
        captured: ts(),
    });
    let err = fb
        .validate()
        .expect_err("formula ref missing from factbase must reject");
    assert!(matches!(err, FactbaseError::UnknownFact { id, .. } if id == "z"));
}
