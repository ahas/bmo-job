//! What a close starts, watched (docs/cli.md, 「`bmo-job job close`」): the check runs in a process group of its
//! own with its output read through pipes, ended with everything under it once it writes nothing for the close's
//! silence limit, and saying where it stands as it runs; and every short command the
//! close asks, ended once it runs past its clock.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use crate::job_close::Clocks;

/// The variable bmo-testbed reads for a file to touch while a command it watches writes but holds its output
/// back; the close counts a touch as its step writing.
pub const HEARD_VARIABLE: &str = "BMO_TESTBED_HEARD";

/// The longest a line kept as a step's last is, so a step writing without line ends cannot grow it unbounded.
const LONGEST_LINE: usize = 4096;
/// The longest a process's command line is when the close names it.
const LONGEST_NAME: usize = 160;
/// How many of a step's last whole lines are kept for what the close says of a step that did not pass.
const KEPT_LINES: usize = 64;

/// Where a watched step reads the time its silence, its lines saying where it stands and the grace after a TERM
/// are measured on. A close for a job reads `RealTime`; a test moves its own, so what it checks never hangs on how
/// fast a loaded machine runs.
pub trait StepTime: Send + Sync + std::fmt::Debug {
  /// The steady time the step's output, its lines and the grace are measured on.
  fn now(&self) -> Instant;
  /// The wall time a touch of the file handed in `HEARD_VARIABLE` is measured against.
  fn wall(&self) -> SystemTime;
}

/// The machine's own time.
#[derive(Debug)]
pub struct RealTime;

impl StepTime for RealTime {
  fn now(&self) -> Instant {
    Instant::now()
  }

  fn wall(&self) -> SystemTime {
    SystemTime::now()
  }
}

// How long ago `at` was on `time`.
fn since(time: &dyn StepTime, at: Instant) -> Duration {
  time.now().saturating_duration_since(at)
}

/// How a step the close ran ended short of passing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepEnd {
  /// It could not be started, or it ended without passing.
  Failed(String),
  /// The close ended it for writing nothing: how long, its last line and what was still running.
  Hung(String),
}

/// The step a close is running now, which a stop ends with everything under it. A step is started and held
/// under one lock, so a stop either finds it held or comes first and keeps it from starting.
#[derive(Clone, Debug, Default)]
pub struct Running(Arc<Mutex<Holding>>);

// The leader of the step running now, and whether the close has been told to stop.
#[derive(Debug, Default)]
struct Holding {
  leader: Option<u32>,
  stopped: bool,
}

impl Running {
  fn holding(&self) -> std::sync::MutexGuard<'_, Holding> {
    self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
  }

  // Starts `command` and holds it as the step running now; refused once the close has been told to stop.
  fn start(&self, command: &mut Command) -> std::io::Result<Child> {
    let mut holding = self.holding();

    if holding.stopped {
      return Err(std::io::Error::other("the close was told to stop"));
    }

    let child = command.spawn()?;
    holding.leader = Some(child.id());

    Ok(child)
  }

  fn release(&self) {
    self.holding().leader = None;
  }

  /// Ends the step running now, if one is, with every process under it, names what it ended, and keeps every
  /// later step from starting.
  pub fn take_down(&self, clocks: &Clocks) -> Vec<String> {
    let held = {
      let mut holding = self.holding();
      holding.stopped = true;
      holding.leader
    };

    held.map(|leader| end_tree(leader, clocks)).unwrap_or_default()
  }
}

/// Runs `command`, `what` as the close names it, to its end in a process group of its own, its output written on
/// as it comes. Once neither its output nor the file it is handed in `HEARD_VARIABLE` has moved for the silence
/// limit, every process under it is ended and the step is `Hung`. Every `standing_every`, `say` is handed one
/// line saying where it stands. A step `running` has been told to stop on is not started, and fails.
pub fn run_step(
  command: Command,
  what: &str,
  clocks: &Clocks,
  running: &Running,
  say: &dyn Fn(&str),
) -> Result<(), StepEnd> {
  run_step_kept(command, what, clocks, running, say).0
}

/// Runs a step as `run_step` does, and answers besides how it ended its last whole lines, oldest first, so a
/// caller reads what a step that did not pass said of itself.
pub fn run_step_kept(
  mut command: Command,
  what: &str,
  clocks: &Clocks,
  running: &Running,
  say: &dyn Fn(&str),
) -> (Result<(), StepEnd>, Vec<String>) {
  let not_started = |why: std::io::Error| StepEnd::Failed(format!("{what} could not be started: {why}"));
  let heard = match HeardFile::make() {
    Ok(heard) => heard,
    Err(why) => return (Err(not_started(why)), Vec::new()),
  };

  command
    .process_group(0)
    .env(HEARD_VARIABLE, heard.path())
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

  let mut child = match running.start(&mut command) {
    Ok(child) => child,
    Err(why) => return (Err(not_started(why)), Vec::new()),
  };
  let leader = child.id();
  let time = clocks.time.as_ref();
  let moved = Arc::new(Mutex::new(Moved::new(time.now())));
  let (read_out, readers) = mpsc::channel();

  if let Some(stdout) = child.stdout.take() {
    let read = Reading::new(&moved, &clocks.time, &read_out);
    pass_on(stdout, std::io::stdout, read);
  }
  if let Some(stderr) = child.stderr.take() {
    let read = Reading::new(&moved, &clocks.time, &read_out);
    pass_on(stderr, std::io::stderr, read);
  }
  drop(read_out);

  let started = time.now();
  let mut said = started;

  let status = loop {
    match child.try_wait() {
      Ok(Some(status)) => break Some(status),
      Ok(None) => {}
      Err(_) => break None,
    }

    let quiet = quiet_for(&moved, &heard, time);

    if quiet >= clocks.silence {
      let still = end_tree(leader, clocks);
      let _ = child.wait();
      running.release();

      let hung = StepEnd::Hung(format!(
        "{what} wrote nothing for {} and was ended; its last line: {}; still running: {}",
        spoken(clocks.silence),
        last_line(&moved),
        if still.is_empty() { "nothing".to_owned() } else { still.join("; ") }
      ));

      return (Err(hung), kept_lines(&moved));
    }

    if since(time, said) >= clocks.standing_every {
      say(&format!(
        "job-close: {what} has run {}; its output last moved {} ago: {}",
        spoken(since(time, started)),
        spoken(quiet),
        last_line(&moved)
      ));
      said = time.now();
    }

    std::thread::sleep(clocks.look);
  };

  // Whatever the step left standing in its group is a leftover of it: ended, so its output closes and is read
  // whole, each reader waited for no longer than the grace.
  end_group(leader, clocks);
  for _ in 0..2 {
    if !ended_within(&readers, clocks) {
      break;
    }
  }
  running.release();

  let ended = match status {
    Some(status) if status.success() => Ok(()),
    Some(_) => Err(StepEnd::Failed(format!("{what} did not pass"))),
    None => Err(StepEnd::Failed(format!("{what} could not be waited on"))),
  };

  (ended, kept_lines(&moved))
}

// Whether one of the step's readers says it has ended within the grace.
fn ended_within(readers: &mpsc::Receiver<()>, clocks: &Clocks) -> bool {
  let time = clocks.time.as_ref();
  let from = time.now();

  loop {
    match readers.recv_timeout(clocks.look.min(clocks.grace)) {
      Ok(()) => return true,
      Err(RecvTimeoutError::Disconnected) => return false,
      Err(RecvTimeoutError::Timeout) if since(time, from) >= clocks.grace => return false,
      Err(RecvTimeoutError::Timeout) => {}
    }
  }
}

/// The output of a short command the close asks, its standard input closed; one still running once `within` has
/// passed is killed and read as failed.
pub fn output_within(command: &mut Command, within: Duration) -> std::io::Result<Output> {
  let child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
  let pid = child.id();
  let (done, ended) = mpsc::channel();

  std::thread::spawn(move || {
    let _ = done.send(child.wait_with_output());
  });

  match ended.recv_timeout(within) {
    Ok(output) => output,
    Err(_) => {
      signal(pid as i32, libc::SIGKILL);
      // The wait ends once the process is gone; one whose output another holds open is given up on.
      let _ = ended.recv_timeout(within);

      Err(std::io::Error::new(
        ErrorKind::TimedOut,
        format!("it ran past {} and was ended", spoken(within)),
      ))
    }
  }
}

/// A span as a line says it: whole seconds, or milliseconds below one.
pub fn spoken(span: Duration) -> String {
  if span < Duration::from_secs(1) {
    format!("{}ms", span.as_millis())
  } else {
    format!("{}s", span.as_secs())
  }
}

// When a step's output last moved, its last line, the line under way where one is, else the last whole one, and
// its last whole lines.
struct Moved {
  at: Instant,
  partial: Vec<u8>,
  last: String,
  kept: VecDeque<String>,
}

impl Moved {
  fn new(at: Instant) -> Self {
    Self {
      at,
      partial: Vec::new(),
      last: String::new(),
      kept: VecDeque::with_capacity(KEPT_LINES),
    }
  }

  fn take(&mut self, bytes: &[u8], at: Instant) {
    self.at = at;

    for &byte in bytes {
      if byte == b'\n' || byte == b'\r' {
        let line = String::from_utf8_lossy(&self.partial).trim().to_owned();

        if !line.is_empty() {
          if self.kept.len() == KEPT_LINES {
            self.kept.pop_front();
          }
          self.kept.push_back(line.clone());
          self.last = line;
        }
        self.partial.clear();
      } else if self.partial.len() < LONGEST_LINE {
        self.partial.push(byte);
      }
    }
  }

  fn last_line(&self) -> String {
    let under_way = String::from_utf8_lossy(&self.partial).trim().to_owned();

    match (under_way.is_empty(), self.last.is_empty()) {
      (false, _) => under_way,
      (true, false) => self.last.clone(),
      (true, true) => "nothing written".to_owned(),
    }
  }
}

fn last_line(moved: &Mutex<Moved>) -> String {
  moved.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).last_line()
}

fn kept_lines(moved: &Mutex<Moved>) -> Vec<String> {
  moved
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .kept
    .iter()
    .cloned()
    .collect()
}

// How long the step has written nothing and touched nothing.
fn quiet_for(moved: &Mutex<Moved>, heard: &HeardFile, time: &dyn StepTime) -> Duration {
  let since_output = since(time, moved.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).at);

  heard
    .touched_ago(time.wall())
    .map_or(since_output, |since_touch| since_output.min(since_touch))
}

// What a reader of one of the step's outputs keeps: where the output moved and when, read on the step's time,
// and the channel it says it has ended on.
struct Reading {
  moved: Arc<Mutex<Moved>>,
  time: Arc<dyn StepTime>,
  ended: mpsc::Sender<()>,
}

impl Reading {
  fn new(moved: &Arc<Mutex<Moved>>, time: &Arc<dyn StepTime>, ended: &mpsc::Sender<()>) -> Self {
    Self {
      moved: Arc::clone(moved),
      time: Arc::clone(time),
      ended: ended.clone(),
    }
  }
}

// Reads one of the step's outputs to its end, writing each piece on to the close's own as it comes, and says it
// has ended.
fn pass_on<R, W, S>(mut from: R, sink: S, reading: Reading)
where
  R: Read + Send + 'static,
  W: Write,
  S: Fn() -> W + Send + 'static,
{
  std::thread::spawn(move || {
    let mut piece = [0_u8; 8192];

    loop {
      match from.read(&mut piece) {
        Ok(0) => break,
        Ok(read) => {
          let mut to = sink();
          let _ = to.write_all(&piece[..read]);
          let _ = to.flush();
          let at = reading.time.now();
          reading
            .moved
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take(&piece[..read], at);
        }
        Err(error) if error.kind() == ErrorKind::Interrupted => {}
        Err(_) => break,
      }
    }

    let _ = reading.ended.send(());
  });
}

// The file the step is handed to touch, taken away once the step is done.
struct HeardFile(PathBuf);

impl HeardFile {
  fn make() -> std::io::Result<Self> {
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let path = std::env::temp_dir().join(format!("bmo-job-close-heard-{}-{stamp}", std::process::id()));

    std::fs::File::create(&path)?;
    Ok(Self(path))
  }

  fn path(&self) -> &Path {
    &self.0
  }

  fn touched_ago(&self, now: SystemTime) -> Option<Duration> {
    let touched = std::fs::metadata(&self.0).and_then(|meta| meta.modified()).ok()?;

    Some(now.duration_since(touched).unwrap_or_default())
  }
}

impl Drop for HeardFile {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(&self.0);
  }
}

// A process as the kernel lists it.
struct Listed {
  pid: u32,
  parent: u32,
  group: u32,
  gone: bool,
}

// Every process the kernel lists.
fn listed() -> Vec<Listed> {
  let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };

  entries
    .filter_map(Result::ok)
    .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
    .filter_map(listed_one)
    .collect()
}

// The processes of a step: its leader, every process below it, and every process in its group, the close's own
// process left out.
fn tree_of(leader: u32) -> Vec<u32> {
  let all = listed();
  let own = std::process::id();
  let mut found = vec![leader];
  let mut at = 0;

  while at < found.len() {
    let parent = found[at];

    for one in &all {
      if one.parent == parent && !found.contains(&one.pid) {
        found.push(one.pid);
      }
    }
    at += 1;
  }

  for one in &all {
    if one.group == leader && !found.contains(&one.pid) {
      found.push(one.pid);
    }
  }

  found.retain(|pid| *pid != own && standing(*pid));
  found
}

fn standing(pid: u32) -> bool {
  listed_one(pid).is_some_and(|one| !one.gone)
}

// One process, read from `/proc/<pid>/stat`. That line is the kernel's own, not a general format a library
// reads: its second field, the command's name, is set in parentheses and may hold any byte, so the fields are
// taken from past its last closing parenthesis.
fn listed_one(pid: u32) -> Option<Listed> {
  let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
  let mut fields = stat.get(stat.rfind(')')? + 1..)?.split_whitespace();
  let state = fields.next()?;

  Some(Listed {
    pid,
    parent: fields.next()?.parse().ok()?,
    group: fields.next()?.parse().ok()?,
    gone: state == "Z" || state == "X",
  })
}

// A process named as a line says it: its number and its command line.
fn named(pid: u32) -> String {
  let line = std::fs::read(format!("/proc/{pid}/cmdline"))
    .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " ").trim().to_owned())
    .unwrap_or_default();
  let line: String = line.chars().take(LONGEST_NAME).collect();

  if line.is_empty() { format!("{pid}") } else { format!("{pid} {line}") }
}

// Ends a step with everything under it: each process sent a TERM, and a KILL to whatever still stands once the
// grace has passed. Answers what was running as it began.
fn end_tree(leader: u32, clocks: &Clocks) -> Vec<String> {
  let members = tree_of(leader);
  let names = members.iter().map(|pid| named(*pid)).collect();

  for pid in &members {
    signal(*pid as i32, libc::SIGTERM);
  }
  signal(-(leader as i32), libc::SIGTERM);

  let time = clocks.time.as_ref();
  let from = time.now();

  while members.iter().any(|pid| standing(*pid)) && since(time, from) < clocks.grace {
    std::thread::sleep(clocks.look.min(clocks.grace));
  }

  for pid in members.iter().filter(|pid| standing(**pid)) {
    signal(*pid as i32, libc::SIGKILL);
  }
  signal(-(leader as i32), libc::SIGKILL);

  names
}

// Ends what a step that has ended left in its group. Its own process is reaped already, so only the group is
// signalled: its number stays the group's while any member of it stands, and names nobody else.
fn end_group(leader: u32, clocks: &Clocks) {
  let in_group = || listed().iter().any(|one| one.group == leader && !one.gone);

  if !in_group() {
    return;
  }

  signal(-(leader as i32), libc::SIGTERM);

  let time = clocks.time.as_ref();
  let from = time.now();

  while in_group() && since(time, from) < clocks.grace {
    std::thread::sleep(clocks.look.min(clocks.grace));
  }

  signal(-(leader as i32), libc::SIGKILL);
}

fn signal(target: i32, sent: i32) {
  // SAFETY: a signal to a process, or with a negative number to a process group, this close started.
  unsafe { libc::kill(target, sent) };
}
