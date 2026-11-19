mod discover;
mod leftove;
mod model;
mod remove;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use model::*;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

#[derive(PartialEq)]
enum Mode {
    List,
    Search,
    Confirm { force: bool },
    Leftovers,
    Related,
    Report,
}

struct State {
    programs: Vec<Program>,
    filtered: Vec<usize>,
    list: ListState,
    query: String,
    mode: Mode,
    traces: Vec<(Trace, bool)>,
    tlist: ListState,
    log: Vec<String>,
    status: String,
    show_system: bool,
}

impl State {
    fn new() -> Self {
        let mut s = State {
            programs: discover::load(),
            filtered: vec![],
            list: ListState::default(),
            query: String::new(),
            mode: Mode::List,
            traces: vec![],
            tlist: ListState::default(),
            log: vec![],
            status: String::new(),
            show_system: true,
        };
        s.refilter();
        s
    }
    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.programs.len())
            .filter(|&i| {
                let p = &self.programs[i];
                (self.show_system || !p.system)
                    && (q.is_empty()
                        || p.name.to_lowercase().contains(&q)
                        || p.publisher
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&q))
            })
            .collect();
        self.list.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
    }
    fn current(&self) -> Option<&Program> {
        self.list
            .selected()
            .and_then(|i| self.filtered.get(i))
            .map(|&i| &self.programs[i])
    }
}

fn step(l: &mut ListState, len: usize, d: isize) {
    if len == 0 {
        return;
    }
    let i = l.selected().unwrap_or(0) as isize + d;
    l.select(Some(i.clamp(0, len as isize - 1) as usize));
}

fn ui(f: &mut Frame, s: &mut State) {
    let [body, help] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(1)]).areas(f.area());
    let hl = Style::new().add_modifier(Modifier::REVERSED);

    match s.mode {
        Mode::Leftovers | Mode::Related => {
            let related = s.mode == Mode::Related;
            let items: Vec<ListItem> = s
                .traces
                .iter()
                .map(|(t, on)| {
                    if related {
                        ListItem::new(t.to_string())
                    } else {
                        ListItem::new(format!("[{}] {}", if *on { 'x' } else { ' ' }, t))
                    }
                })
                .collect();
            let title = if related {
                format!(" Related locations: {} (files + registry) ", s.traces.len())
            } else {
                format!(" Leftovers found: {} ", s.traces.len())
            };
            f.render_stateful_widget(
                List::new(items)
                    .block(Block::bordered().title(title))
                    .highlight_style(hl),
                body,
                &mut s.tlist,
            );
        }
        Mode::Report => {
            let text = s.log.join("\n");
            f.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(" Result ")),
                body,
            );
        }
        _ => {
            let [top, det] =
                Layout::vertical([Constraint::Min(3), Constraint::Length(7)]).areas(body);
            let items: Vec<ListItem> = s
                .filtered
                .iter()
                .map(|&i| {
                    let p = &s.programs[i];
                    ListItem::new(format!(
                        "{:<5}{:<48} {:<14} {}",
                        if p.system { "[SYS]" } else { "" },
                        trunc(&p.name, 48),
                        trunc(p.version.as_deref().unwrap_or("-"), 14),
                        trunc(p.publisher.as_deref().unwrap_or("-"), 30)
                    ))
                })
                .collect();
            let title = format!(
                " Programs ({}/{})  system apps: {}  filter: {} ",
                s.filtered.len(),
                s.programs.len(),
                if s.show_system { "shown" } else { "hidden" },
                s.query
            );
            f.render_stateful_widget(
                List::new(items)
                    .block(Block::bordered().title(title))
                    .highlight_style(hl),
                top,
                &mut s.list,
            );
            let d = match s.current() {
                Some(p) => format!(
                    "Name:      {}{}\nPublisher: {}\nLocation:  {}\nSize:      {} MB\nUninstall: {}\nKey:       {}\\{}",
                    p.name,
                    if p.system { "  [system component]" } else { "" },
                    p.publisher.as_deref().unwrap_or("-"),
                    p.install_location
                        .as_ref()
                        .map_or("-".into(), |l| l.display().to_string()),
                    p.size_kb / 1024,
                    p.uninstall,
                    p.hive.short(),
                    p.key_name
                ),
                None => "No program selected".into(),
            };
            f.render_widget(
                Paragraph::new(d)
                    .wrap(Wrap { trim: true })
                    .block(Block::bordered().title(" Details ")),
                det,
            );
            if let Mode::Confirm { force } = s.mode {
                let name = s.current().map_or("?", |p| p.name.as_str());
                let sys = s.current().map_or(false, |p| p.system);
                let warn = if sys {
                    "SYSTEM COMPONENT - Windows or other apps may depend on it! "
                } else {
                    ""
                };
                let msg = if force {
                    format!(
                        "{warn}FORCE REMOVE '{name}'? Skips its uninstaller, scans leftovers only. (y/N)"
                    )
                } else {
                    format!("{warn}Uninstall '{name}' and then scan for leftovers? (y/N)")
                };
                f.render_widget(
                    Paragraph::new(msg).style(Style::new().fg(Color::Yellow)),
                    help,
                );
                return;
            }
        }
    }
    let h = match s.mode {
        Mode::List => {
            "↑↓ move  Enter uninstall  f force-remove  s show/hide system  i find related  / search  r reload  q quit"
        }
        Mode::Search => "type to filter  Enter/Esc done",
        Mode::Leftovers => "↑↓ move  Space toggle  a all/none  Enter DELETE selected  Esc cancel",
        Mode::Related => "↑↓ move  w save report to .txt  Enter select for deletion  Esc back",
        Mode::Report => "any key to return",
        Mode::Confirm { .. } => "",
    };
    let line = if s.status.is_empty() {
        h.to_string()
    } else {
        s.status.clone()
    };
    f.render_widget(
        Paragraph::new(line).style(Style::new().fg(Color::Cyan)),
        help,
    );
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}

fn begin(t: &mut DefaultTerminal, s: &mut State, force: bool) -> std::io::Result<()> {
    let Some(p) = s.current().cloned() else {
        return Ok(());
    };
    s.log.clear();
    if !force {
        s.status = format!(
            "Running uninstaller for '{}' ... (waiting for it and its child processes)",
            p.name
        );
        t.draw(|f| ui(f, s))?;
        match remove::run_uninstaller(&p) {
            Ok(()) => s.log.push("Uninstaller finished.".into()),
            Err(e) => s.log.push(format!("Uninstaller failed to start: {e}")),
        }
    }
    s.status = "Scanning for leftovers ...".into();
    t.draw(|f| ui(f, s))?;
    s.traces = leftove::scan(&p).into_iter().map(|t| (t, true)).collect();
    s.status.clear();
    if s.traces.is_empty() {
        s.log.push("No leftovers found.".into());
        s.mode = Mode::Report;
    } else {
        s.tlist.select(Some(0));
        s.mode = Mode::Leftovers;
    }
    Ok(())
}

fn find(t: &mut DefaultTerminal, s: &mut State) -> std::io::Result<()> {
    let Some(p) = s.current().cloned() else {
        return Ok(());
    };
    s.status = format!("Searching files and registry related to '{}' ...", p.name);
    t.draw(|f| ui(f, s))?;
    s.traces = leftove::find_related(&p)
        .into_iter()
        .map(|x| (x, true))
        .collect();
    s.status.clear();
    s.tlist
        .select(if s.traces.is_empty() { None } else { Some(0) });
    s.mode = Mode::Related;
    Ok(())
}

fn save_report(s: &State) -> String {
    let name = s.current().map_or("app".to_string(), |p| {
        p.name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect()
    });
    let file = format!("{name}_related.txt");
    let text: Vec<String> = s.traces.iter().map(|(t, _)| t.to_string()).collect();
    match std::fs::write(&file, text.join("\r\n")) {
        Ok(()) => format!("Saved {} entries to {file}", text.len()),
        Err(e) => format!("Could not write {file}: {e}"),
    }
}

fn finish(s: &mut State) {
    let mut ok = 0;
    for (t, on) in &s.traces {
        if !*on {
            continue;
        }
        match remove::delete(t) {
            Ok(m) => {
                ok += 1;
                s.log.push(format!("OK   {t}  ({m})"));
            }
            Err(e) => s.log.push(format!("FAIL {t}  ({e})")),
        }
    }
    s.log
        .insert(0, format!("Processed {ok} item(s) successfully.\n"));
    s.mode = Mode::Report;
}

fn run(t: &mut DefaultTerminal) -> std::io::Result<()> {
    let mut s = State::new();
    loop {
        t.draw(|f| ui(f, &mut s))?;
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        let n = s.filtered.len();
        match s.mode {
            Mode::List => match k.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => step(&mut s.list, n, 1),
                KeyCode::Up | KeyCode::Char('k') => step(&mut s.list, n, -1),
                KeyCode::PageDown => step(&mut s.list, n, 10),
                KeyCode::PageUp => step(&mut s.list, n, -10),
                KeyCode::Char('s') => {
                    s.show_system = !s.show_system;
                    s.refilter();
                }
                KeyCode::Char('i') if s.current().is_some() => find(t, &mut s)?,
                KeyCode::Char('/') => s.mode = Mode::Search,
                KeyCode::Char('r') => {
                    s.programs = discover::load();
                    s.refilter();
                }
                KeyCode::Enter if s.current().is_some() => s.mode = Mode::Confirm { force: false },
                KeyCode::Char('f') if s.current().is_some() => {
                    s.mode = Mode::Confirm { force: true }
                }
                _ => {}
            },
            Mode::Search => match k.code {
                KeyCode::Enter | KeyCode::Esc => s.mode = Mode::List,
                KeyCode::Backspace => {
                    s.query.pop();
                    s.refilter();
                }
                KeyCode::Char(c) => {
                    s.query.push(c);
                    s.refilter();
                }
                _ => {}
            },
            Mode::Confirm { force } => {
                if matches!(k.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    begin(t, &mut s, force)?;
                } else {
                    s.mode = Mode::List;
                }
            }
            Mode::Leftovers => {
                let len = s.traces.len();
                match k.code {
                    KeyCode::Down | KeyCode::Char('j') => step(&mut s.tlist, len, 1),
                    KeyCode::Up | KeyCode::Char('k') => step(&mut s.tlist, len, -1),
                    KeyCode::Char(' ') => {
                        if let Some(i) = s.tlist.selected() {
                            s.traces[i].1 = !s.traces[i].1;
                        }
                    }
                    KeyCode::Char('a') => {
                        let v = !s.traces.iter().all(|x| x.1);
                        s.traces.iter_mut().for_each(|x| x.1 = v);
                    }
                    KeyCode::Enter => finish(&mut s),
                    KeyCode::Esc => s.mode = Mode::List,
                    _ => {}
                }
            }
            Mode::Related => {
                let len = s.traces.len();
                match k.code {
                    KeyCode::Down | KeyCode::Char('j') => step(&mut s.tlist, len, 1),
                    KeyCode::Up | KeyCode::Char('k') => step(&mut s.tlist, len, -1),
                    KeyCode::PageDown => step(&mut s.tlist, len, 10),
                    KeyCode::PageUp => step(&mut s.tlist, len, -10),
                    KeyCode::Char('w') => s.status = save_report(&s),
                    KeyCode::Enter if len > 0 => {
                        s.status.clear();
                        s.mode = Mode::Leftovers;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        s.status.clear();
                        s.mode = Mode::List;
                    }
                    _ => {}
                }
            }
            Mode::Report => {
                s.programs = discover::load();
                s.refilter();
                s.mode = Mode::List;
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut term = ratatui::init();
    let r = run(&mut term);
    ratatui::restore();
    r
}
