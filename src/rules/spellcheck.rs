//! Spell-check identifiers, comments, docs, UDL and CHANGELOG
//!
//! Docs and examples: `rules/spellcheck/`.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;

use crate::helpers::docs_deps::{comment_lines, is_out_of_scope};
use crate::rule::{Area, Context, Findings, Fix, Kind, Rule};
use crate::source::{RustFile, TextFile};
use crate::workspace::is_test_path;

pub static RULE: Rule = Rule {
    id: "spellcheck",
    area: Area::Docs,
    title: "Spell-check identifiers, comments, docs, UDL and CHANGELOG",
    kind: Kind::Code,
    clippy: &[],
    in_tests: false,
    check,
};

/// Misspellings that are never right, with their fix. Only unambiguous ones:
/// spelling variants (`cancelation`, `cachable`), British spellings and
/// apostrophe-less contractions are left alone.
const MISSPELLINGS: &[(&str, &str)] = &[
    ("abilty", "ability"),
    ("acceptible", "acceptable"),
    ("accesible", "accessible"),
    ("accidently", "accidentally"),
    ("accomodate", "accommodate"),
    ("acheive", "achieve"),
    ("adress", "address"),
    ("aggresive", "aggressive"),
    ("agressive", "aggressive"),
    ("algoritm", "algorithm"),
    ("allready", "already"),
    ("alogrithm", "algorithm"),
    ("alredy", "already"),
    ("amoung", "among"),
    ("anomoly", "anomaly"),
    ("anwser", "answer"),
    ("apparant", "apparent"),
    ("apparantly", "apparently"),
    ("appearence", "appearance"),
    ("applicaiton", "application"),
    ("aquire", "acquire"),
    ("arguement", "argument"),
    ("arguements", "arguments"),
    ("assomption", "assumption"),
    ("assosiated", "associated"),
    ("asynchonous", "asynchronous"),
    ("attribtue", "attribute"),
    ("authentification", "authentication"),
    ("availabe", "available"),
    ("availble", "available"),
    ("avaliable", "available"),
    ("backgound", "background"),
    ("basicly", "basically"),
    ("becasue", "because"),
    ("becuase", "because"),
    ("beggining", "beginning"),
    ("begining", "beginning"),
    ("beleive", "believe"),
    ("belive", "believe"),
    ("benifit", "benefit"),
    ("boostrapped", "bootstrapped"),
    ("bootstraped", "bootstrapped"),
    ("borken", "broken"),
    ("boundry", "boundary"),
    ("buisness", "business"),
    ("calender", "calendar"),
    ("catagory", "category"),
    ("charachter", "character"),
    ("charater", "character"),
    ("choosen", "chosen"),
    ("collumn", "column"),
    ("comming", "coming"),
    ("commited", "committed"),
    ("commiting", "committing"),
    ("comparision", "comparison"),
    ("compatability", "compatibility"),
    ("compatable", "compatible"),
    ("completly", "completely"),
    ("componentes", "components"),
    ("concurent", "concurrent"),
    ("conditon", "condition"),
    ("configuraton", "configuration"),
    ("consistant", "consistent"),
    ("containg", "containing"),
    ("convienient", "convenient"),
    ("correspondance", "correspondence"),
    ("corresponing", "corresponding"),
    ("cuase", "cause"),
    ("curently", "currently"),
    ("currenly", "currently"),
    ("cyphertext", "ciphertext"),
    ("debuging", "debugging"),
    ("decison", "decision"),
    ("definately", "definitely"),
    ("defintion", "definition"),
    ("defualt", "default"),
    ("dependancies", "dependencies"),
    ("dependancy", "dependency"),
    ("depricated", "deprecated"),
    ("derrived", "derived"),
    ("descibe", "describe"),
    ("desription", "description"),
    ("destory", "destroy"),
    ("detatch", "detach"),
    ("devce", "device"),
    ("develoment", "development"),
    ("differnt", "different"),
    ("diffrent", "different"),
    ("dissapear", "disappear"),
    ("efficent", "efficient"),
    ("elasped", "elapsed"),
    ("eligable", "eligible"),
    ("encryted", "encrypted"),
    ("enviornment", "environment"),
    ("enviroment", "environment"),
    ("equivelant", "equivalent"),
    ("existance", "existence"),
    ("existant", "existent"),
    ("exmple", "example"),
    ("experiance", "experience"),
    ("experiements", "experiments"),
    ("experiemnt", "experiment"),
    ("explicitely", "explicitly"),
    ("expresion", "expression"),
    ("extenstion", "extension"),
    ("failiure", "failure"),
    ("failue", "failure"),
    ("familar", "familiar"),
    ("fileds", "fields"),
    ("finaly", "finally"),
    ("flexibilty", "flexibility"),
    ("follwing", "following"),
    ("folowing", "following"),
    ("foriegn", "foreign"),
    ("formating", "formatting"),
    ("fragement", "fragment"),
    ("freqently", "frequently"),
    ("fucntion", "function"),
    ("funtion", "function"),
    ("futher", "further"),
    ("garantee", "guarantee"),
    ("gaurantee", "guarantee"),
    ("generaly", "generally"),
    ("grammer", "grammar"),
    ("guarentee", "guarantee"),
    ("happend", "happened"),
    ("heirarchy", "hierarchy"),
    ("hierachy", "hierarchy"),
    ("identifer", "identifier"),
    ("identiifer", "identifier"),
    ("immediatly", "immediately"),
    ("implementaion", "implementation"),
    ("implemention", "implementation"),
    ("implmentation", "implementation"),
    ("incomming", "incoming"),
    ("incompatable", "incompatible"),
    ("inconsistant", "inconsistent"),
    ("indentifier", "identifier"),
    ("independant", "independent"),
    ("infomation", "information"),
    ("inital", "initial"),
    ("initalize", "initialize"),
    ("initializiation", "initialization"),
    ("instaed", "instead"),
    ("instatiated", "instantiated"),
    ("intead", "instead"),
    ("interpet", "interpret"),
    ("interupt", "interrupt"),
    ("intial", "initial"),
    ("invaild", "invalid"),
    ("irrelevent", "irrelevant"),
    ("isntead", "instead"),
    ("knowlege", "knowledge"),
    ("langauge", "language"),
    ("lenght", "length"),
    ("libary", "library"),
    ("lisence", "license"),
    ("maintainance", "maintenance"),
    ("maintanance", "maintenance"),
    ("maintence", "maintenance"),
    ("managment", "management"),
    ("manualy", "manually"),
    ("mesage", "message"),
    ("messsage", "message"),
    ("mispell", "misspell"),
    ("mispelled", "misspelled"),
    ("modifer", "modifier"),
    ("muliple", "multiple"),
    ("neccessary", "necessary"),
    ("necesary", "necessary"),
    ("necessery", "necessary"),
    ("neglible", "negligible"),
    ("nothign", "nothing"),
    ("noticable", "noticeable"),
    ("occassion", "occasion"),
    ("occurance", "occurrence"),
    ("occured", "occurred"),
    ("occurence", "occurrence"),
    ("occuring", "occurring"),
    ("ommit", "omit"),
    ("ommited", "omitted"),
    ("ommitted", "omitted"),
    ("orginal", "original"),
    ("orignal", "original"),
    ("overriden", "overridden"),
    ("paramater", "parameter"),
    ("paramter", "parameter"),
    ("particularily", "particularly"),
    ("peform", "perform"),
    ("perfomance", "performance"),
    ("performace", "performance"),
    ("permanant", "permanent"),
    ("perserve", "preserve"),
    ("persistant", "persistent"),
    ("posible", "possible"),
    ("possibilty", "possibility"),
    ("potentialy", "potentially"),
    ("preceeding", "preceding"),
    ("prefered", "preferred"),
    ("preferrable", "preferable"),
    ("presense", "presence"),
    ("previosly", "previously"),
    ("privilage", "privilege"),
    ("priviledge", "privilege"),
    ("probablly", "probably"),
    ("procces", "process"),
    ("proccess", "process"),
    ("programatically", "programmatically"),
    ("propogate", "propagate"),
    ("protocal", "protocol"),
    ("pseduo", "pseudo"),
    ("publically", "publicly"),
    ("reccomend", "recommend"),
    ("recieve", "receive"),
    ("recieved", "received"),
    ("reciever", "receiver"),
    ("recieving", "receiving"),
    ("recomend", "recommend"),
    ("refered", "referred"),
    ("refering", "referring"),
    ("regresion", "regression"),
    ("relevence", "relevance"),
    ("relevent", "relevant"),
    ("remeber", "remember"),
    ("repective", "respective"),
    ("replacment", "replacement"),
    ("reponse", "response"),
    ("requirment", "requirement"),
    ("reseting", "resetting"),
    ("resouce", "resource"),
    ("resouces", "resources"),
    ("responsbility", "responsibility"),
    ("respository", "repository"),
    ("retreive", "retrieve"),
    ("retrive", "retrieve"),
    ("seperate", "separate"),
    ("seperated", "separated"),
    ("seperately", "separately"),
    ("seperator", "separator"),
    ("shbang", "shebang"),
    ("similiar", "similar"),
    ("specfic", "specific"),
    ("specifc", "specific"),
    ("succesful", "successful"),
    ("succesfully", "successfully"),
    ("successfull", "successful"),
    ("sucess", "success"),
    ("sufficent", "sufficient"),
    ("sugestions", "suggestions"),
    ("supress", "suppress"),
    ("suprise", "surprise"),
    ("syncronous", "synchronous"),
    ("targetted", "targeted"),
    ("targetting", "targeting"),
    ("temporarly", "temporarily"),
    ("tempory", "temporary"),
    ("threshhold", "threshold"),
    ("transfered", "transferred"),
    ("truely", "truly"),
    ("typicaly", "typically"),
    ("unecessary", "unnecessary"),
    ("unkown", "unknown"),
    ("unneccessary", "unnecessary"),
    ("unsuccesful", "unsuccessful"),
    ("untill", "until"),
    ("usefull", "useful"),
    ("usualy", "usually"),
    ("vaild", "valid"),
    ("varaible", "variable"),
    ("verison", "version"),
    ("versionned", "versioned"),
    ("visable", "visible"),
    ("wether", "whether"),
    ("wich", "which"),
    ("wierd", "weird"),
    ("wrappper", "wrapper"),
    ("writen", "written"),
    ("writting", "writing"),
];

fn fix_for(word: &str) -> Option<&'static str> {
    let lower = word.to_ascii_lowercase();
    MISSPELLINGS
        .binary_search_by(|(typo, _)| typo.cmp(&lower.as_str()))
        .ok()
        .map(|i| MISSPELLINGS[i].1)
}

/// The words of a token: `num_recieved` → `num`, `recieved`;
/// `SeperatorKind` → `Seperator`, `Kind`.
fn words(token: &str) -> Vec<&str> {
    word_spans(token).into_iter().map(|(_, w)| w).collect()
}

/// [`words`] with the byte offset of each word in `text`.
fn word_spans(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for part in text.split(|c: char| !c.is_ascii_alphabetic()) {
        // `split` yields slices of `text`.
        let base = part.as_ptr() as usize - text.as_ptr() as usize;
        let mut start = 0;
        let chars: Vec<(usize, char)> = part.char_indices().collect();
        for w in 1..chars.len() {
            let (i, c) = chars[w];
            let prev = chars[w - 1].1;
            let next_lower = chars
                .get(w + 1)
                .is_some_and(|(_, n)| n.is_ascii_lowercase());
            if c.is_ascii_uppercase()
                && (prev.is_ascii_lowercase() || next_lower && prev.is_ascii_uppercase())
            {
                out.push((base + start, &part[start..i]));
                start = i;
            }
        }
        if start < part.len() {
            out.push((base + start, &part[start..]));
        }
    }
    out.retain(|(_, w)| w.len() >= 4);
    out
}

/// Misspelled words of one file: the first place each one appears, and how
/// often it does.
#[derive(Default)]
struct Typos {
    /// lowercase word -> (line, word as written, token, kind of token, lines)
    found: BTreeMap<String, (usize, String, String, &'static str, BTreeSet<usize>)>,
    /// (lowercase word, line) -> how the word was seen on that line.
    seen: BTreeMap<(String, usize), Seen>,
    /// Lowercase identifiers holding a misspelled word.
    identifiers: BTreeSet<String>,
}

/// How a misspelled word was seen on one line.
#[derive(Default, Clone, Copy)]
struct Seen {
    /// Times in prose: comments, docs, string literals, Markdown.
    prose: usize,
    identifier: bool,
}

impl Typos {
    fn scan(&mut self, line: usize, text: &str, what: &'static str) {
        for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            for word in words(token) {
                if fix_for(word).is_none() {
                    continue;
                }
                let lower = word.to_ascii_lowercase();
                let seen = self.seen.entry((lower.clone(), line)).or_default();
                if what.is_empty() {
                    seen.prose += 1;
                } else {
                    seen.identifier = true;
                    self.identifiers.insert(token.to_ascii_lowercase());
                }
                let entry = self.found.entry(lower).or_insert_with(|| {
                    (
                        line,
                        word.to_string(),
                        token.to_string(),
                        what,
                        BTreeSet::new(),
                    )
                });
                // Keep the first line; on that line prefer the identifier.
                if line < entry.0 || line == entry.0 && !what.is_empty() && entry.3.is_empty() {
                    entry.0 = line;
                    entry.1 = word.to_string();
                    entry.2 = token.to_string();
                    entry.3 = what;
                }
                entry.4.insert(line);
            }
        }
    }

    /// Report each word at its first line. `text` is the file's text;
    /// `identifiers` are the misspelled identifiers of the whole workspace.
    fn report(self, path: &str, text: &str, identifiers: &BTreeSet<String>, out: &mut Findings) {
        for (lower, (line, word, token, what, lines)) in &self.found {
            let count = lines.len();
            let fix = fix_for(lower).unwrap_or_default();
            // Keep the word's capitalization: `Componentes` -> `Components`.
            let fix = match word.chars().next() {
                Some(c) if c.is_ascii_uppercase() => capitalized(fix),
                _ => fix.to_string(),
            };
            let context = if what.is_empty() || token == word {
                String::new()
            } else {
                format!(" (in {what} `{token}`)")
            };
            let more = if count > 1 {
                format!(", and on {} more line(s) of this file", count - 1)
            } else {
                String::new()
            };
            let message = format!("`{word}` is misspelled{context}{more}: write `{fix}`");
            match self.line_fix(path, text, lower, *line, identifiers) {
                Some(edit) => out.add_fix(path, *line, message, edit),
                None => out.add(path, *line, message),
            }
        }
    }

    /// The line with the word corrected, when that is safe: the word is never
    /// an identifier in this file, and every place it takes on the line is
    /// prose (not code, a quoted value or a URL).
    fn line_fix(
        &self,
        path: &str,
        text: &str,
        lower: &str,
        line: usize,
        identifiers: &BTreeSet<String>,
    ) -> Option<Fix> {
        let in_code = self
            .seen
            .range((lower.to_string(), 0)..=(lower.to_string(), usize::MAX))
            .any(|(_, s)| s.identifier);
        let seen = self.seen.get(&(lower.to_string(), line))?;
        if in_code || seen.identifier {
            return None;
        }
        let raw = text.lines().nth(line.checked_sub(1)?)?;
        let spans = occurrences(raw, lower);
        // Every place on the line was seen as prose: none sits in code the
        // scan skipped (Kotlin, Python, UDL declarations, ...).
        if spans.is_empty() || spans.len() != seen.prose {
            return None;
        }
        let udl_comment = raw.find("//");
        let mut fixed = raw.to_string();
        for &(start, end) in spans.iter().rev() {
            if path.ends_with(".udl") && udl_comment.is_none_or(|c| start < c) {
                return None;
            }
            if in_backticks(raw, start) || in_quoted_value(raw, start) {
                return None;
            }
            let (token_start, token_end) = token_bounds(raw, start, end);
            if (token_start, token_end) != (start, end) {
                // Part of a compound name: it must not name code.
                let token = &raw[token_start..token_end];
                if identifiers.contains(&token.to_ascii_lowercase())
                    || text.matches(token).count() != 1
                {
                    return None;
                }
            }
            let written = &raw[start..end];
            fixed.replace_range(start..end, &cased(fix_for(lower)?, written));
        }
        Some(Fix::replace_line(line, fixed))
    }
}

/// `fix` capitalized.
fn capitalized(fix: &str) -> String {
    let mut chars = fix.chars();
    chars
        .next()
        .map(|f| f.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// `fix` with the case of `written`: `RECIEVED` -> `RECEIVED`,
/// `Recieved` -> `Received`.
fn cased(fix: &str, written: &str) -> String {
    if written.len() > 1 && written.chars().all(|c| c.is_ascii_uppercase()) {
        fix.to_ascii_uppercase()
    } else if written.starts_with(|c: char| c.is_ascii_uppercase()) {
        capitalized(fix)
    } else {
        fix.to_string()
    }
}

/// Byte ranges of the misspelled word `lower` on `line`, as the scan splits
/// words, outside URLs.
fn occurrences(line: &str, lower: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for chunk in line.split_whitespace() {
        if chunk.contains("://") {
            continue;
        }
        let base = chunk.as_ptr() as usize - line.as_ptr() as usize;
        for (offset, word) in word_spans(chunk) {
            if word.eq_ignore_ascii_case(lower) {
                out.push((base + offset, base + offset + word.len()));
            }
        }
    }
    out
}

/// Whether byte `at` of `line` is inside a `` `code` `` span.
fn in_backticks(line: &str, at: usize) -> bool {
    line[..at].matches('`').count() % 2 == 1
}

/// Whether byte `at` of `line` is inside a double-quoted value rather than a
/// quoted sentence: `"recieved"`, a SQL string, or a string that goes on
/// past the line.
fn in_quoted_value(line: &str, at: usize) -> bool {
    let quotes: Vec<usize> = line
        .match_indices('"')
        .map(|(i, _)| i)
        .filter(|&i| !line[..i].ends_with('\\'))
        .collect();
    let before = quotes.iter().filter(|&&i| i < at).count();
    if before % 2 == 0 {
        return false;
    }
    let open = quotes[before - 1];
    let Some(&close) = quotes.get(before) else {
        return true;
    };
    let quoted = &line[open + 1..close];
    const SQL: &[&str] = &[
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE ", " FROM ", " WHERE ",
    ];
    !quoted.contains(char::is_whitespace) || SQL.iter().any(|k| quoted.contains(k))
}

/// The identifier-like token (`[A-Za-z0-9_]+`) around `start..end`.
fn token_bounds(line: &str, start: usize, end: usize) -> (usize, usize) {
    let word_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let token_start = line[..start]
        .rfind(|c: char| !word_char(c))
        .map(|i| i + line[i..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(0);
    let token_end = line[end..]
        .find(|c: char| !word_char(c))
        .map(|i| end + i)
        .unwrap_or(line.len());
    (token_start, token_end)
}

/// Text with URLs blanked out: a typo in someone else's URL is not ours.
fn without_urls(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| !w.contains("://"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn check(cx: &Context, out: &mut Findings) {
    let mut scanned = Vec::new();
    for file in cx.ws.files() {
        let p = file.path.as_str();
        if is_out_of_scope(p) || is_test_like(p) || p.contains("/dumps/") {
            continue;
        }
        let mut typos = Typos::default();
        match file.extension() {
            "md" => markdown(file, &mut typos),
            "rs" => {
                if let Some(rust) = cx.ws.rust(p) {
                    rust_file(rust, &mut typos);
                }
            }
            "udl" => {
                for (line, text) in file.lines() {
                    typos.scan(line, &without_urls(text), "");
                }
            }
            "kt" | "kts" | "swift" | "py" | "sh" | "gradle" | "yml" | "yaml" | "toml" => {
                for (line, text) in comment_lines(p, &file.text) {
                    typos.scan(line, &without_urls(&text), "");
                }
            }
            _ => {}
        }
        scanned.push((file, typos));
    }
    // A fix must not rename, in prose, an identifier some other file uses.
    let identifiers: BTreeSet<String> = scanned
        .iter()
        .flat_map(|(_, t)| t.identifiers.iter().cloned())
        .collect();
    for (file, typos) in scanned {
        typos.report(&file.path, &file.text, &identifiers, out);
    }
}

/// Test code and fixtures in any language (`src/test/java`, `tests/`, ...).
fn is_test_like(path: &str) -> bool {
    is_test_path(path)
        || path
            .split('/')
            .any(|s| matches!(s, "test" | "androidTest" | "Tests" | "UITests"))
}

/// Comments, doc comments, string literals and identifiers.
fn rust_file(file: &RustFile, typos: &mut Typos) {
    for (line, text) in comment_lines(&file.path, &file.text) {
        if !file.in_test(line) {
            typos.scan(line, &without_urls(&text), "");
        }
    }
    tokens(file, file.ast.to_token_stream(), typos);
}

/// Identifiers and string literals (doc attributes repeat the comments,
/// which is harmless: a word is reported once per file).
fn tokens(file: &RustFile, stream: TokenStream, typos: &mut Typos) {
    for tree in stream {
        match tree {
            TokenTree::Group(g) => {
                // `#[doc = "…"]` repeats a doc comment, already checked.
                let doc = matches!(g.stream().into_iter().next(), Some(TokenTree::Ident(i)) if i == "doc");
                if !doc {
                    tokens(file, g.stream(), typos);
                }
            }
            TokenTree::Ident(id) => {
                let line = id.span().start().line;
                if !file.in_test(line) {
                    typos.scan(line, &id.to_string(), "identifier");
                }
            }
            TokenTree::Literal(lit) => {
                let text = lit.to_string();
                let line = lit.span().start().line;
                let string =
                    text.starts_with('"') || text.starts_with("r\"") || text.starts_with("r#");
                if string && !file.in_test(line) {
                    typos.scan(line, &without_urls(&text), "");
                }
            }
            TokenTree::Punct(_) => {}
        }
    }
}

/// Markdown prose outside code fences; in CHANGELOG.md only the unreleased
/// section, since released ones are history.
fn markdown(file: &TextFile, typos: &mut Typos) {
    let changelog = file.file_name() == "CHANGELOG.md";
    let mut fenced = false;
    for (line, text) in file.lines() {
        if changelog && text.starts_with("# v") && text.contains("(_") {
            break;
        }
        if text.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            typos.scan(line, &without_urls(text), "");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MISSPELLINGS, RULE};
    use crate::testing::check;

    const PATH: &str = "components/suggest/src/ids.rs";

    #[test]
    fn dictionary_is_sorted_and_clean() {
        assert!(MISSPELLINGS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(MISSPELLINGS
            .iter()
            .all(|(t, f)| t != f && t.chars().all(|c| c.is_ascii_lowercase())));
    }

    #[test]
    fn flags_dont() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/spellcheck/dont.rs"))],
        );
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 4], "{found:#?}");
        assert!(found[1].message.contains("identifier `num_recieved`"));
    }

    #[test]
    fn accepts_do() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/spellcheck/do.rs"))],
        );
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn camel_case_strings_and_urls() {
        let text = "pub enum SeperatorKind { A }\nfn f() { log(\"occured twice\"); }\n// See https://example.com/recieve for details.\n";
        let found = check(&RULE, &[(PATH, text)]);
        let lines: Vec<usize> = found.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![1, 2], "{found:#?}");
    }

    /// The single edit of a finding's fix: `(line, remove, insert)`.
    fn edit(f: &crate::rule::Finding) -> (usize, usize, Vec<String>) {
        let fix = f.fix.as_ref().expect("a fix");
        assert_eq!(fix.edits.len(), 1, "{fix:#?}");
        let e = &fix.edits[0];
        (e.line, e.remove, e.insert.clone())
    }

    #[test]
    fn fixes_prose_but_not_identifiers() {
        let found = check(
            &RULE,
            &[(PATH, include_str!("../../rules/spellcheck/dont.rs"))],
        );
        assert_eq!(
            edit(&found[0]),
            (
                1,
                1,
                vec![
                    "/// Parses a comma-separated list of record ids. // flagged: separated".into()
                ]
            )
        );
        // Renaming `num_recieved` on one line would break the code.
        assert!(found[1].fix.is_none(), "{found:#?}");
    }

    #[test]
    fn fixes_keep_case_and_skip_code_quotes_and_values() {
        let text = "// RECIEVED and Recieved here.\n\
            // `recieve()` is the call\n\
            fn f() { log(\"occured twice\"); }\n\
            // \"L'Assomption\" => \"LAssomption\"\n\
            fn g() { m(\"seperate\"); }\n";
        let found = check(&RULE, &[(PATH, text)]);
        let fixes: Vec<(usize, bool)> = found.iter().map(|f| (f.line, f.fix.is_some())).collect();
        assert_eq!(
            fixes,
            vec![(1, true), (2, false), (3, true), (4, false), (5, false)],
            "{found:#?}"
        );
        assert_eq!(edit(&found[0]).2, vec!["// RECEIVED and Received here."]);
        assert_eq!(
            edit(&found[2]).2,
            vec!["fn f() { log(\"occurred twice\"); }"]
        );
    }

    #[test]
    fn code_the_scan_skips_is_left_alone() {
        // In Python only comments are read: `recieve()` is code.
        let py = "x = recieve()  # recieve the data\ny = 1  # perserve_order is a serde feature\n";
        let found = check(&RULE, &[("automation/a.py", py)]);
        assert!(found[0].fix.is_none(), "{found:#?}");
        assert_eq!(
            edit(&found[1]).2,
            vec!["y = 1  # preserve_order is a serde feature"]
        );
    }

    #[test]
    fn changelog_history_and_tests_are_skipped() {
        let changelog = "# v161.0 (In progress)\n\n- Fixed a seperate bug.\n\n# v160.0 (_2026-10-22_)\n\n- An occured typo.\n";
        let test = "fn t() { assert_eq!(query(\"recieve\"), 1); }\n";
        let found = check(
            &RULE,
            &[
                ("CHANGELOG.md", changelog),
                ("components/x/tests/t.rs", test),
                ("components/x/android/src/test/java/T.kt", "// recieve\n"),
            ],
        );
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].line, 3);
    }
}
