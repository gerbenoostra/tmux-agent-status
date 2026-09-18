//! Reading and rewriting a `window-status-format` line in a tmux config file.
//!
//! Narrow on purpose. The parser recognises the shape `docs/register.md` asks for and
//! refuses everything else, because the fallback - printing the line for the
//! user to edit - is the state they are in today and costs them nothing. A
//! general tmux parser is a project; a parser that knows when to stop is a
//! feature.
//!
//! Nothing here touches tmux or the filesystem: every function is a pure
//! function over strings, which is what makes the corpus of real config lines
//! in the tests below cheap enough to be exhaustive.

use std::ops::Range;

/// The term 001 and `docs/register.md` specify, and the only thing this module inserts.
pub const TERM: &str = "#{?@agent_status, #{@agent_status},}";

/// The two options that must carry the term.
///
/// Both, always: a term in only one of them makes the glyph vanish the moment
/// the window becomes current.
pub const OPTIONS: [&str; 2] = ["window-status-format", "window-status-current-format"];

/// tmux's compiled-in default, used only when tmux cannot be run at all.
///
/// The right answer comes from asking this tmux (`probe`), because the default
/// has changed between versions. This is the answer for a machine with no tmux
/// on it yet, which is a legitimate order to do things in.
pub const FALLBACK_DEFAULT: &str = "#I:#W#{?window_flags,#{window_flags}, }";

/// The commands that assign an option. tmux accepts unique prefixes of these
/// too; a prefix falls to the manual path rather than being guessed at.
const SET_COMMANDS: [&str; 4] = ["set", "set-option", "setw", "set-window-option"];

/// How a word was written, which is how it has to be written back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quoting {
    Bare,
    Single,
    Double,
}

/// Why a line that assigns one of our options will not be rewritten.
///
/// Each of these is a line tmux reads differently from the way a naive rewrite
/// would, so the only safe move is to hand it back to the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A `;` outside quotes: the line carries a second command.
    SecondCommand,
    /// An unterminated quote, or quoting this parser cannot read.
    Unreadable,
    /// The option is named but nothing is assigned to it.
    NoValue,
    /// A word after the value. tmux abandons the whole config file over this,
    /// so the line is already broken and is not ours to touch.
    ExtraArgument,
}

impl Refusal {
    /// What to tell the user about a line we are handing back.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::SecondCommand => "the line carries a second command after a `;`",
            Refusal::Unreadable => "the line's quoting cannot be read safely",
            Refusal::NoValue => "the option is named but no value is assigned",
            Refusal::ExtraArgument => "the line has an argument after the value",
        }
    }
}

/// What one logical config line turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Candidate {
    /// Not an assignment of either format option.
    NotOurs,
    /// An assignment of one of them that will not be rewritten. The option is
    /// carried anyway, because which one it is still decides what happens to
    /// the *other* one.
    Refused { option: String, why: Refusal },
    /// An assignment this module can read and splice.
    Line(Box<FormatLine>),
}

impl Candidate {
    /// The line, when the candidate is one this module can rewrite.
    pub fn line(self) -> Option<FormatLine> {
        match self {
            Candidate::Line(line) => Some(*line),
            _ => None,
        }
    }

    /// Why the line is being handed back, when it is one of ours and refused.
    pub fn refusal(self) -> Option<Refusal> {
        match self {
            Candidate::Refused { why, .. } => Some(why),
            _ => None,
        }
    }

    /// Which option this line assigns, whether or not it can be rewritten.
    ///
    /// A refused line still assigns its option, and pretending otherwise makes
    /// the caller treat the option as unset and append a second assignment
    /// beside a perfectly good one.
    pub fn option(&self) -> Option<&str> {
        match self {
            Candidate::Line(line) => Some(&line.option),
            Candidate::Refused { option, .. } => Some(option),
            Candidate::NotOurs => None,
        }
    }
}

/// A `set ... window-status-format <value>` line, ready to be spliced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatLine {
    /// Which of the two options this line assigns.
    pub option: String,
    /// The value exactly as the file spells it, without its delimiters. The
    /// splice works on these bytes, so the user's own escapes survive
    /// untouched.
    pub raw: String,
    pub quoting: Quoting,
    /// The whole logical line, which the rewrite reproduces verbatim except
    /// for `span`.
    line: String,
    /// Byte span of the value word - delimiters included - within `line`.
    span: Range<usize>,
}

impl FormatLine {
    /// Whether this value already reads `@agent_status`.
    ///
    /// Wherever the user put the term, they put it there on purpose.
    pub fn already_registered(&self) -> bool {
        references_agent_status(&self.raw)
    }

    /// The value with the term spliced in, still in the file's own spelling.
    pub fn spliced_value(&self) -> String {
        splice(&self.raw)
    }

    /// The whole line rewritten, or `None` when the value cannot be requoted.
    ///
    /// Only the value word changes: the command name, the flags and any
    /// trailing whitespace are reproduced byte for byte, so the diff a user
    /// reads is the one they were shown.
    pub fn spliced_line(&self) -> Option<String> {
        let value = requote(&self.spliced_value(), self.quoting)?;
        let mut out = String::with_capacity(self.line.len() + TERM.len() + 2);
        out.push_str(&self.line[..self.span.start]);
        out.push_str(&value);
        out.push_str(&self.line[self.span.end..]);
        Some(out)
    }
}

/// Whether a format value references our option, as a token rather than a
/// substring.
///
/// A user's unrelated `#{@agent_status_colour}` is not our term, and matching
/// the full literal term instead would miss anyone who dropped the space or
/// wrapped it in styling and then register a second copy beside their first.
pub fn references_agent_status(value: &str) -> bool {
    let name = "@agent_status";
    let mut rest = value;
    while let Some(at) = rest.find(name) {
        let after = &rest[at + name.len()..];
        match after.chars().next() {
            Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
            _ => return true,
        }
        rest = after;
    }
    false
}

/// Insert the term at the position 001 specifies.
///
/// Immediately before the first `#{?window_flags`, which puts it after the
/// name segment and outside any truncation; at the end of the value when there
/// is no flags term to sit in front of.
pub fn splice(raw: &str) -> String {
    match raw.find("#{?window_flags") {
        Some(at) => format!("{}{TERM}{}", &raw[..at], &raw[at..]),
        None => format!("{raw}{TERM}"),
    }
}

/// A `set -g <option> <value>` line, for an option with no assignment of its
/// own to splice.
///
/// `None` when the value cannot be quoted safely, which sends the step to the
/// manual path like any other value this module will not write.
pub fn assignment(option: &str, value: &str) -> Option<String> {
    Some(format!(
        "set -g {option} {}\n",
        requote(value, Quoting::Bare)?
    ))
}

/// The words of one logical config line, unquoted.
///
/// `None` for a comment, a blank line, a line carrying a second command, or
/// quoting this module will not read - the same tokenizer that reads a format
/// line, so the two never disagree about where a word ends.
pub fn words(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    match tokenize(line) {
        (tokens, Stop::End) => Some(tokens.into_iter().map(|token| token.text).collect()),
        _ => None,
    }
}

/// Read one logical config line.
pub fn parse(line: &str) -> Candidate {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Candidate::NotOurs;
    }

    let (tokens, stop) = tokenize(line);
    let mut words = tokens.iter();

    let Some(command) = words.next() else {
        return Candidate::NotOurs;
    };
    if command.quoting != Quoting::Bare || !SET_COMMANDS.contains(&command.text.as_str()) {
        return Candidate::NotOurs;
    }

    let mut next = words.next();
    while let Some(word) = next {
        if !is_flag(word) {
            break;
        }
        // `-t` takes a target: attached as `-tother`, or the following word
        // when the cluster ends on it, as `-t other` and `-gt other` do.
        if word.text.ends_with('t') {
            words.next();
        }
        next = words.next();
    }

    let Some(option) = next else {
        return Candidate::NotOurs;
    };
    if !OPTIONS.contains(&option.text.as_str()) {
        return Candidate::NotOurs;
    }

    // From here the line is one of ours, so every exit is a refusal the user
    // hears about rather than a line silently passed over.
    let refuse = |why| Candidate::Refused {
        option: option.text.clone(),
        why,
    };
    match stop {
        Stop::SecondCommand => return refuse(Refusal::SecondCommand),
        Stop::Unreadable => return refuse(Refusal::Unreadable),
        Stop::End => {}
    }

    let Some(value) = words.next() else {
        return refuse(Refusal::NoValue);
    };
    if words.next().is_some() {
        return refuse(Refusal::ExtraArgument);
    }

    Candidate::Line(Box::new(FormatLine {
        option: option.text.clone(),
        raw: line[value.inner.clone()].to_owned(),
        quoting: value.quoting,
        line: line.to_owned(),
        span: value.word.clone(),
    }))
}

/// A physical line, or a run of them joined by trailing backslashes.
///
/// tmux joins a continuation into one command before running it, so the parser
/// has to see what tmux sees. A rewrite replaces the whole run with one line,
/// which is a formatting change the confirmation discloses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalLine {
    pub text: String,
    /// 0-based index of the first physical line of the run.
    pub first: usize,
    /// 0-based index of its last physical line, inclusive.
    pub last: usize,
}

/// Split a config file into the lines tmux would execute.
pub fn logical_lines(text: &str) -> Vec<LogicalLine> {
    let mut out: Vec<LogicalLine> = Vec::new();
    let mut pending: Option<LogicalLine> = None;

    for (index, physical) in text.lines().enumerate() {
        let continues = continuation(physical);
        let piece = if continues {
            &physical[..physical.len() - 1]
        } else {
            physical
        };
        match &mut pending {
            Some(line) => {
                line.text.push_str(piece);
                line.last = index;
            }
            None => {
                pending = Some(LogicalLine {
                    text: piece.to_owned(),
                    first: index,
                    last: index,
                });
            }
        }
        if !continues {
            out.push(pending.take().expect("a line was just started"));
        }
    }
    // A file whose last line ends in a backslash has nothing to join it to.
    out.extend(pending);
    out
}

/// Whether a physical line is continued by the next one.
///
/// An odd number of trailing backslashes continues; an even number is escaped
/// backslashes that end the line.
fn continuation(line: &str) -> bool {
    line.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1
}

/// Where the tokenizer stopped, which is half of what the parser decides on.
///
/// The `;` test has to be lexical: a semicolon inside quotes is part of the
/// value and loads perfectly well, so only one the tokenizer meets outside
/// quotes separates commands. That is the same walk that finds the value,
/// which is why one function answers both questions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    End,
    SecondCommand,
    Unreadable,
}

/// One word of a tmux command line.
#[derive(Clone, Debug)]
struct Token {
    /// The word including any delimiters.
    word: Range<usize>,
    /// Its contents without them.
    inner: Range<usize>,
    quoting: Quoting,
    /// The contents with quoting removed, for comparing against a name.
    text: String,
}

fn tokenize(line: &str) -> (Vec<Token>, Stop) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let offset = |at: usize| chars.get(at).map_or(line.len(), |(byte, _)| *byte);
    let mut tokens = Vec::new();
    let mut at = 0;

    loop {
        while matches!(chars.get(at), Some((_, ' ' | '\t'))) {
            at += 1;
        }
        let Some((_, first)) = chars.get(at).copied() else {
            return (tokens, Stop::End);
        };
        if first == ';' {
            return (tokens, Stop::SecondCommand);
        }

        let start = at;
        let (inner, quoting) = match first {
            '\'' => {
                at += 1;
                let inner_start = at;
                while !matches!(chars.get(at), None | Some((_, '\''))) {
                    at += 1;
                }
                if chars.get(at).is_none() {
                    return (tokens, Stop::Unreadable);
                }
                let inner = offset(inner_start)..offset(at);
                at += 1;
                (inner, Quoting::Single)
            }
            '"' => {
                at += 1;
                let inner_start = at;
                loop {
                    match chars.get(at) {
                        None => return (tokens, Stop::Unreadable),
                        Some((_, '"')) => break,
                        Some((_, '\\')) => at += 2,
                        Some(_) => at += 1,
                    }
                }
                let inner = offset(inner_start)..offset(at);
                at += 1;
                (inner, Quoting::Double)
            }
            _ => {
                while let Some((_, c)) = chars.get(at) {
                    match c {
                        ' ' | '\t' | ';' => break,
                        // tmux opens a quoted section wherever a quote appears,
                        // so `it's` is an unterminated quote and `a'b'c` is
                        // concatenation. Neither is a bare value holding a
                        // quote, and this parser reads neither.
                        '\'' | '"' => return (tokens, Stop::Unreadable),
                        '\\' => at += 2,
                        _ => at += 1,
                    }
                }
                // A trailing backslash walked the cursor past the end.
                if at > chars.len() {
                    return (tokens, Stop::Unreadable);
                }
                (offset(start)..offset(at), Quoting::Bare)
            }
        };

        // Adjacent quoting - `a'b'` - is a word this parser does not read.
        if !matches!(chars.get(at), None | Some((_, ' ' | '\t' | ';'))) {
            return (tokens, Stop::Unreadable);
        }

        let word = offset(start)..offset(at);
        tokens.push(Token {
            text: unquote(&line[inner.clone()], quoting),
            word,
            inner,
            quoting,
        });
    }
}

/// A word's value with its escapes resolved, for comparing against a name.
fn unquote(inner: &str, quoting: Quoting) -> String {
    match quoting {
        // Nothing is an escape inside single quotes.
        Quoting::Single => inner.to_owned(),
        Quoting::Bare | Quoting::Double => {
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            out
        }
    }
}

fn is_flag(word: &Token) -> bool {
    word.quoting == Quoting::Bare && word.text.starts_with('-') && word.text.len() > 1
}

/// Quote a value the way the file spelled it, or say it cannot be done.
///
/// A bare value must always be requoted, because the term contains a space and
/// a bare value ends at the first space - tmux discards such a line silently
/// and keeps the option's default, with the line still sitting in the config
/// looking correct.
fn requote(raw: &str, quoting: Quoting) -> Option<String> {
    match quoting {
        // The value came out from between these delimiters, so it cannot
        // contain one, and the term contains neither.
        Quoting::Single => Some(format!("'{raw}'")),
        Quoting::Double => Some(format!("\"{raw}\"")),
        Quoting::Bare => {
            // Single quotes would make an escape literal and change the value.
            if raw.contains('\\') {
                None
            } else if !raw.contains('\'') {
                Some(format!("'{raw}'"))
            } else if !raw.contains(['"', '$', '`']) {
                Some(format!("\"{raw}\""))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> FormatLine {
        parse(text).line().expect("parses as a format line")
    }

    fn refusal(text: &str) -> Refusal {
        parse(text).refusal().expect("is refused")
    }

    #[test]
    fn a_refused_line_still_says_which_option_it_assigns() {
        // Otherwise the caller reads the option as unset and appends a second
        // assignment beside a perfectly good one.
        let refused = parse("set -g window-status-format 'x' ; set -g status on");
        assert_eq!(refused.option(), Some("window-status-format"));
        assert_eq!(
            parse("set -g window-status-current-format 'x'").option(),
            Some("window-status-current-format")
        );
        assert_eq!(parse("set -g status on").option(), None);
    }

    #[test]
    fn the_term_is_the_one_register_md_documents() {
        assert_eq!(TERM, "#{?@agent_status, #{@agent_status},}");
    }

    #[test]
    fn a_single_quoted_line_is_read_and_spliced() {
        let parsed = line("set -g window-status-format '#I:#W#{?window_flags,#{window_flags}, }'");
        assert_eq!(parsed.option, "window-status-format");
        assert_eq!(parsed.quoting, Quoting::Single);
        assert_eq!(parsed.raw, FALLBACK_DEFAULT);
        assert!(!parsed.already_registered());
        assert_eq!(
            parsed.spliced_line().unwrap(),
            format!(
                "set -g window-status-format '#I:#W{TERM}#{{?window_flags,#{{window_flags}}, }}'"
            )
        );
    }

    #[test]
    fn every_command_spelling_and_flag_form_is_read() {
        for text in [
            "set -g window-status-format 'x'",
            "set-option -gw window-status-format 'x'",
            "setw window-status-format 'x'",
            "set-window-option -g window-status-current-format 'x'",
            "set -g -t other window-status-format 'x'",
            "set -gt other window-status-format 'x'",
            "  \tset  -g   window-status-format   'x'",
        ] {
            assert_eq!(line(text).raw, "x", "line {text:?}");
        }
    }

    #[test]
    fn a_line_that_assigns_something_else_is_not_ours() {
        for text in [
            "set -g status-left 'x'",
            "bind-key -n C-a send-prefix",
            "# set -g window-status-format 'x'",
            "",
            "   ",
            "set -g",
            "'set' -g window-status-format 'x'",
            "se -g window-status-format 'x'",
            "; set -g window-status-format 'x'",
        ] {
            assert_eq!(parse(text), Candidate::NotOurs, "line {text:?}");
        }
    }

    #[test]
    fn the_term_position_is_before_the_flags_term_or_at_the_end() {
        assert_eq!(
            splice("#I:#W#{?window_flags,x, }"),
            format!("#I:#W{TERM}#{{?window_flags,x, }}")
        );
        assert_eq!(splice("#I:#W"), format!("#I:#W{TERM}"));
    }

    #[test]
    fn a_value_that_already_carries_the_term_is_left_alone() {
        let parsed =
            line("set -g window-status-format '#I:#W#{?@agent_status, #{@agent_status},}'");
        assert!(parsed.already_registered());
    }

    #[test]
    fn a_reference_is_a_token_and_not_a_substring() {
        assert!(references_agent_status("#{@agent_status}"));
        assert!(references_agent_status("#{?@agent_status,x,}"));
        assert!(references_agent_status("#{@agent_status"));
        assert!(!references_agent_status("#{@agent_status_colour}"));
        assert!(!references_agent_status("#{@agent_statuses}"));
        assert!(!references_agent_status("#{@agent_status2}"));
        // The first hit is a prefix of a longer name; the second is genuine.
        assert!(references_agent_status(
            "#{@agent_status_colour}#{@agent_status}"
        ));
        assert!(!references_agent_status("#I:#W"));
    }

    #[test]
    fn a_semicolon_inside_quotes_is_part_of_the_value() {
        let parsed = line("set -g window-status-format 'SEMI;INSIDE#{?window_flags,x, }'");
        assert_eq!(parsed.raw, "SEMI;INSIDE#{?window_flags,x, }");
        assert!(parsed.spliced_line().unwrap().contains("SEMI;INSIDE"));
    }

    #[test]
    fn a_semicolon_outside_quotes_refuses_the_line() {
        assert_eq!(
            refusal("set -g window-status-format 'x' ; set -g status-left 'y'"),
            Refusal::SecondCommand
        );
        assert_eq!(
            refusal("set -g window-status-format x; set -g status-left y"),
            Refusal::SecondCommand
        );
    }

    #[test]
    fn quoting_the_parser_cannot_read_refuses_the_line() {
        for text in [
            "set -g window-status-format 'unterminated",
            "set -g window-status-format \"unterminated",
            "set -g window-status-format a'b'c",
            "set -g window-status-format 'a'b",
            "set -g window-status-format it's",
            "set -g window-status-format trailing\\",
        ] {
            assert_eq!(refusal(text), Refusal::Unreadable, "line {text:?}");
        }
    }

    #[test]
    fn a_named_option_with_nothing_assigned_is_refused() {
        assert_eq!(refusal("set -g window-status-format"), Refusal::NoValue);
    }

    #[test]
    fn a_word_after_the_value_is_refused() {
        // tmux abandons the whole config file over this, so the line is
        // already broken and is not ours to rewrite.
        assert_eq!(
            refusal("set -g window-status-format 'x' stray"),
            Refusal::ExtraArgument
        );
    }

    #[test]
    fn a_candidate_answers_only_for_what_it_is() {
        assert!(parse("set -g window-status-format 'x'").refusal().is_none());
        assert!(parse("set -g window-status-format").line().is_none());
    }

    #[test]
    fn every_refusal_says_why() {
        for refusal in [
            Refusal::SecondCommand,
            Refusal::Unreadable,
            Refusal::NoValue,
            Refusal::ExtraArgument,
        ] {
            assert!(!refusal.reason().is_empty(), "{refusal:?} has no reason");
        }
    }

    #[test]
    fn a_double_quoted_value_keeps_its_escapes() {
        let parsed =
            line(r##"set -g window-status-format "#I:#W #(echo \"hi\") #{?window_flags,x, }""##);
        assert_eq!(parsed.quoting, Quoting::Double);
        let out = parsed.spliced_line().unwrap();
        assert!(out.contains(r##"#(echo \"hi\")"##), "escapes lost: {out}");
        assert!(out.contains(TERM), "term missing: {out}");
    }

    #[test]
    fn a_bare_value_is_always_requoted() {
        // The term contains a space and a bare value ends at the first one, so
        // a spliced value left bare produces a line tmux discards in silence.
        let parsed = line("set -g window-status-format plain");
        assert_eq!(parsed.quoting, Quoting::Bare);
        assert_eq!(
            parsed.spliced_line().unwrap(),
            format!("set -g window-status-format 'plain{TERM}'")
        );
    }

    #[test]
    fn a_bare_value_holding_a_space_is_a_line_tmux_already_discards() {
        // Verified on 3.6a: tmux reads the space as an argument separator and
        // throws the line away, keeping the option's default. The line is
        // already broken, so it is not ours to rewrite.
        assert_eq!(
            refusal("set -g window-status-format BARE#{?window_flags,x, }"),
            Refusal::ExtraArgument
        );
    }

    #[test]
    fn a_bare_value_holding_a_backslash_falls_to_the_manual_path() {
        // Single quotes would make the escape literal and change the value.
        assert!(
            line("set -g window-status-format back\\ slash")
                .spliced_line()
                .is_none()
        );
    }

    #[test]
    fn only_the_value_word_changes() {
        // Round-trip property: everything outside the value survives byte for
        // byte, trailing whitespace and odd spacing included.
        let text = "setw  -g\t window-status-format   'x'   ";
        let out = line(text).spliced_line().unwrap();
        assert_eq!(
            out,
            format!("setw  -g\t window-status-format   'x{TERM}'   ")
        );
    }

    #[test]
    fn words_uses_the_same_tokenizer_as_the_parser() {
        assert_eq!(
            words("source-file -q '/odd path/x.conf'"),
            Some(vec![
                "source-file".to_owned(),
                "-q".to_owned(),
                "/odd path/x.conf".to_owned()
            ])
        );
        assert_eq!(words("source-file ~/x.conf").unwrap().len(), 2);
        // A comment, a blank line, a second command and unreadable quoting all
        // decline to answer rather than guessing.
        assert_eq!(words("# source-file x"), None);
        assert_eq!(words("   "), None);
        assert_eq!(words("source-file x; source-file y"), None);
        assert_eq!(words("source-file 'unterminated"), None);
    }

    #[test]
    fn continuations_are_joined_into_the_line_tmux_runs() {
        let text = "set -g window-status-format \\\n'#I:#W'\nset -g status on\n";
        let lines = logical_lines(text);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "set -g window-status-format '#I:#W'");
        assert_eq!((lines[0].first, lines[0].last), (0, 1));
        assert_eq!((lines[1].first, lines[1].last), (2, 2));
        assert_eq!(line(&lines[0].text).raw, "#I:#W");
    }

    #[test]
    fn an_even_run_of_backslashes_ends_the_line() {
        let lines = logical_lines("set -g status-left 'a\\\\'\nset -g status on\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "set -g status-left 'a\\\\'");
    }

    #[test]
    fn a_file_ending_in_a_backslash_still_yields_its_line() {
        let lines = logical_lines("set -g status on\nset -g x \\");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "set -g x ");
        assert_eq!((lines[1].first, lines[1].last), (1, 1));
    }

    #[test]
    fn an_empty_file_has_no_lines() {
        assert!(logical_lines("").is_empty());
    }

    #[test]
    fn a_single_option_can_be_assigned_on_its_own() {
        // The case of a config that sets one of the two and leaves the other
        // on tmux's default.
        assert_eq!(
            assignment(OPTIONS[1], "#I:#W").unwrap(),
            "set -g window-status-current-format '#I:#W'\n"
        );
        assert!(assignment(OPTIONS[0], "it's$HOME").is_none());
    }

    #[test]
    fn a_probed_default_holding_a_quote_is_requoted_around_it() {
        // The default comes back from tmux rather than from a bare word, so it
        // can hold anything; single quotes are the first choice and double
        // quotes the fallback.
        assert!(assignment(OPTIONS[0], "#I:#W").unwrap().contains("'#I:#W'"));
        assert!(assignment(OPTIONS[0], "it's").unwrap().contains("\"it's\""));
    }

    #[test]
    fn a_default_that_defeats_both_quotes_has_no_line() {
        for value in ["it's$HOME", "it's\"quoted\"", "it's`x`", "back\\slash"] {
            assert!(assignment(OPTIONS[0], value).is_none(), "value {value:?}");
        }
    }

    #[test]
    fn a_multibyte_value_does_not_split_a_character() {
        let parsed = line("set -g window-status-format '#I:#W ✅'");
        assert_eq!(parsed.raw, "#I:#W ✅");
        assert!(parsed.spliced_line().unwrap().contains('✅'));
    }
}
