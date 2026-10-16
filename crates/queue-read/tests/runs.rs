//! What one job of a room is labelled by (docs/reading.md, "A room's jobs").

use job_codes::work_lines::JobLabel;
use queue_read::runs::work_label;

#[test]
fn a_run_is_named_by_its_item_and_which_run_of_it_it_is() {
  assert_eq!(
    work_label(Some(9), 1),
    JobLabel::Item {
      id: 9,
      attempt: None
    }
  );
  assert_eq!(
    work_label(Some(9), 2),
    JobLabel::Item {
      id: 9,
      attempt: Some(2)
    }
  );
  assert_eq!(work_label(None, 0), JobLabel::Itemless);
}
