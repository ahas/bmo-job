//! Routes a verb's moment reaches the service by (docs/api.md, "Routes"): the queue told at a moment a verb saw
//! move it, refused where the unit its standing is read through does not answer and with nothing told; and the
//! pinned workers' programs, which are bmo-agency's to stand again and have no route here.

use bmo_job::verbs::built::Built;
use serde_json::json;

use crate::support::server::{Served, code};

#[tokio::test(flavor = "multi_thread")]
async fn the_queue_told_with_nothing_answering_behind_it_is_refused_and_tells_nobody() {
  let served = Served::<Built>::ported().await;
  let (status, body) = served.post("/queue/tell", json!({ "moment": "startup", "repeat": true })).await;

  assert_eq!((status, code(&body)), (502, "agency"));
  assert!(
    !served.receiver.calls().iter().any(|call| call.route == "POST /instructions"),
    "{:?}",
    served.receiver.calls()
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn standing_the_workers_programs_again_is_no_route_of_this_service() {
  let served = Served::<Built>::ported().await;
  let (status, body) = served.post("/workers/stand", json!({ "lockWaitSeconds": 60 })).await;

  assert_eq!((status, code(&body)), (404, "route_absent"));
}
