//! An item's impact grounds and the verdict they come to.

use queue_read::impact::{Fault, ImpactGrounds, ImpactNode, Verdict};

fn grounds(contracts: &[Option<&str>], new_home: bool) -> ImpactGrounds {
  ImpactGrounds {
    nodes: contracts
      .iter()
      .enumerate()
      .map(|(at, contract)| ImpactNode {
        id: format!("node/{at}"),
        contract: contract.map(str::to_owned),
      })
      .collect(),
    new_home,
  }
}

#[test]
fn a_change_inside_nodes_alone_is_a_single_job_however_many_it_reaches() {
  assert_eq!(grounds(&[None, None, None], false).verdict(), Verdict::Single);
}

#[test]
fn one_contract_that_changes_is_a_single_job() {
  assert_eq!(grounds(&[Some("the plan's JSON"), None], false).verdict(), Verdict::Single);
}

#[test]
fn two_contracts_that_change_are_a_co_work() {
  assert_eq!(
    grounds(&[Some("the schema"), Some("the wire format"), None], false).verdict(),
    Verdict::Cowork
  );
}

#[test]
fn a_new_home_is_a_co_work_whatever_the_contracts() {
  assert_eq!(grounds(&[], true).verdict(), Verdict::Cowork);
  assert_eq!(grounds(&[None], true).verdict(), Verdict::Cowork);
}

#[test]
fn written_grounds_read_back_as_they_were() {
  let given = grounds(&[Some("the schema"), None], false);

  assert_eq!(ImpactGrounds::read(&given.written()), Ok(given));
  assert_eq!(
    ImpactGrounds::read(r#"{"nodes":[{"id":"bot/work-orders"}]}"#),
    Ok(ImpactGrounds {
      nodes: vec![ImpactNode {
        id: "bot/work-orders".into(),
        contract: None
      }],
      new_home: false,
    })
  );
}

#[test]
fn grounds_out_of_shape_are_refused() {
  assert!(matches!(ImpactGrounds::read("nodes"), Err(Fault::Shape(_))));
  assert!(matches!(ImpactGrounds::read(r#"{"nodes":[],"extra":1}"#), Err(Fault::Shape(_))));
  assert_eq!(ImpactGrounds::read(r#"{"nodes":[]}"#), Err(Fault::Empty));
  assert_eq!(ImpactGrounds::read(r#"{"nodes":[{"id":" "}]}"#), Err(Fault::Blank));
  assert_eq!(ImpactGrounds::read(r#"{"nodes":[{"id":"a","contract":""}]}"#), Err(Fault::Blank));
  assert_eq!(
    ImpactGrounds::read(r#"{"nodes":[{"id":"a"},{"id":"a"}]}"#),
    Err(Fault::Repeated("a".into()))
  );
}
