//! The state tool's executable: one call handed by the tool host on standard input, answered on standard
//! output (docs/tools.md, "A call").

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
  job_tool_call::answer_standard_io(state_tool::ENTRIES).await
}
