//! What a run came to, queued as an instruction through bmo-receiver for the agent whose turn set it going
//! (docs/queue.md, "Telling a room"): that agent where it still answers the run's room, the room's voice
//! otherwise, and the owner's chat where the room's registration is gone.

use job_receiver::{Instruction, Said};

use super::units::Units;
use crate::Refusal;

/// What a run came to queued for the agent whose turn set it going, naming the run; its id. The
/// instruction is given as it is to be queued, save the agent and the room, which are settled here: a run
/// whose room's registration is gone hands it to the room's work chat instead, saying so to the agent, and
/// neither the removed room's road nor its kind goes with it.
pub async fn queue_for_run(
  units: &dyn Units,
  run: Option<i64>,
  chat: i64,
  mut instruction: Instruction,
) -> Result<i64, Refusal> {
  let room = units.room(chat).await?;

  instruction.run = instruction.run.or(run);

  if !room.registered && room.work_chat != chat {
    if let Said::Order(text) = &instruction.said {
      instruction.said = Said::Order(format!(
        "This came back from work started in room {chat}, whose registration has been removed; nothing goes to that room, so it is told here.\n\n{text}"
      ));
    }

    instruction.via = None;
    instruction.kind = None;
    instruction.chat = Some(room.work_chat);
    instruction.agent = Some(units.answerers(room.work_chat).await?.voice);

    return units.queue_instruction(&instruction).await;
  }

  let asker = match run {
    Some(run) => units.run(run).await?.map(|run| run.agent),
    None => None,
  };
  let answerers = units.answerers(chat).await?;
  let agent = match asker {
    Some(asker) if answerers.answerers.iter().any(|one| one.agent == asker) => asker,
    _ => answerers.voice,
  };

  instruction.chat = Some(chat);
  instruction.agent = Some(agent);

  units.queue_instruction(&instruction).await
}
