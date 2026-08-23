#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandInvocation {
    pub executable: String,
    pub arguments: Vec<String>,
}

pub(crate) fn parse_command_line(source: &str) -> Vec<CommandInvocation> {
    let tokens = tokenize(source);
    let mut invocations = Vec::new();
    let mut words = Vec::new();

    for token in tokens {
        match token {
            Token::Operator(Operator::Separator) => {
                push_invocation(&mut invocations, &mut words);
            }
            Token::Operator(Operator::Redirect) => {
                // The following word is a redirection target, not a command argument.
                words.push(String::new());
            }
            Token::Word(word) => {
                if words.last().is_some_and(String::is_empty) {
                    words.pop();
                } else {
                    words.push(word);
                }
            }
        }
    }
    push_invocation(&mut invocations, &mut words);

    let nested_shells = invocations
        .iter()
        .filter_map(nested_shell_command)
        .flat_map(parse_command_line)
        .collect::<Vec<_>>();
    let substitutions = command_substitutions(source)
        .into_iter()
        .flat_map(|command| parse_command_line(&command));
    invocations.extend(nested_shells.into_iter().chain(substitutions));
    invocations
}

fn push_invocation(invocations: &mut Vec<CommandInvocation>, words: &mut Vec<String>) {
    if words.is_empty() {
        return;
    }

    if let Some(index) = executable_index(words) {
        let executable = basename(&words[index]).to_ascii_lowercase();
        invocations.push(CommandInvocation {
            executable,
            arguments: words.drain(index + 1..).collect(),
        });
    }
    words.clear();
}

fn nested_shell_command(invocation: &CommandInvocation) -> Option<&str> {
    if !matches!(
        invocation.executable.as_str(),
        "sh" | "bash" | "zsh" | "dash" | "fish"
    ) {
        return None;
    }
    invocation
        .arguments
        .iter()
        .position(|argument| {
            argument == "--command"
                || argument
                    .strip_prefix('-')
                    .is_some_and(|options| !options.starts_with('-') && options.contains('c'))
        })
        .and_then(|index| invocation.arguments.get(index + 1))
        .map(String::as_str)
}

fn executable_index(words: &[String]) -> Option<usize> {
    let mut index = 0;
    while index < words.len() && is_assignment(&words[index]) {
        index += 1;
    }
    loop {
        let wrapper = words.get(index).map(|word| basename(word));
        match wrapper {
            Some("command" | "builtin" | "exec" | "nohup") => index += 1,
            Some("env") => {
                index += 1;
                skip_wrapper_options(words, &mut index, &["-u", "--unset", "-C", "--chdir"]);
                while words.get(index).is_some_and(|word| is_assignment(word)) {
                    index += 1;
                }
            }
            Some("sudo") => {
                index += 1;
                skip_wrapper_options(
                    words,
                    &mut index,
                    &[
                        "-u",
                        "--user",
                        "-g",
                        "--group",
                        "-h",
                        "--host",
                        "-p",
                        "--prompt",
                        "-C",
                        "--close-from",
                        "-T",
                        "--command-timeout",
                        "-r",
                        "--role",
                        "-t",
                        "--type",
                    ],
                );
            }
            _ => return (index < words.len()).then_some(index),
        }
    }
}

fn skip_wrapper_options(words: &[String], index: &mut usize, options_with_values: &[&str]) {
    while let Some(argument) = words.get(*index) {
        if argument == "--" {
            *index += 1;
            return;
        }
        if !argument.starts_with('-') || argument == "-" {
            return;
        }
        let option = argument
            .split_once('=')
            .map_or(argument.as_str(), |(name, _)| name);
        *index += 1;
        if !argument.contains('=') && options_with_values.contains(&option) {
            *index += usize::from(*index < words.len());
        }
    }
}

fn command_substitutions(source: &str) -> Vec<String> {
    let characters = source.chars().collect::<Vec<_>>();
    let mut substitutions = Vec::new();
    let mut index = 0;
    let mut quote = None;
    while index < characters.len() {
        let character = characters[index];
        if character == '\'' {
            quote = if quote == Some('\'') {
                None
            } else if quote.is_none() {
                Some('\'')
            } else {
                quote
            };
            index += 1;
            continue;
        }
        if character == '"' {
            quote = if quote == Some('"') {
                None
            } else if quote.is_none() {
                Some('"')
            } else {
                quote
            };
            index += 1;
            continue;
        }
        if quote != Some('\'') && character == '`' {
            let start = index + 1;
            index = start;
            while index < characters.len() && characters[index] != '`' {
                index += 1;
            }
            if index < characters.len() {
                substitutions.push(characters[start..index].iter().collect());
                index += 1;
            }
            continue;
        }
        let parenthesized_start = (quote != Some('\'')
            && matches!(character, '$' | '<' | '>')
            && characters.get(index + 1) == Some(&'('))
        .then_some(index + 2);
        if let Some(start) = parenthesized_start {
            if let Some(end) = parenthesized_substitution_end(&characters, start) {
                substitutions.push(characters[start..end].iter().collect());
                index = end + 1;
            } else {
                index = characters.len();
            }
            continue;
        }
        index += 1;
    }
    substitutions
}

fn parenthesized_substitution_end(characters: &[char], start: usize) -> Option<usize> {
    let mut depth = 1;
    let mut quote = None;
    let mut index = start;
    while index < characters.len() {
        let character = characters[index];
        if character == '\\' && quote != Some('\'') {
            index = index.saturating_add(2);
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = if quote == Some(character) {
                None
            } else if quote.is_none() {
                Some(character)
            } else {
                quote
            };
            index += 1;
            continue;
        }
        if quote != Some('\'') && character == '$' && characters.get(index + 1) == Some(&'(') {
            depth += 1;
            index += 2;
            continue;
        }
        if quote.is_none() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

fn basename(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Separator,
    Redirect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Operator(Operator),
}

fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut chars = source.chars().peekable();
    let mut quote = None;

    while let Some(character) = chars.next() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            } else if character == '\\' && active_quote == '"' {
                if let Some(escaped) = chars.next() {
                    word.push(escaped);
                }
            } else {
                word.push(character);
            }
            continue;
        }

        match character {
            '\'' | '"' => quote = Some(character),
            '\\' => {
                if let Some(escaped) = chars.next() {
                    word.push(escaped);
                }
            }
            ' ' | '\t' | '\r' => push_word(&mut tokens, &mut word),
            '\n' | ';' | '|' | '&' => {
                push_word(&mut tokens, &mut word);
                if chars.peek() == Some(&character) {
                    chars.next();
                }
                tokens.push(Token::Operator(Operator::Separator));
            }
            '<' | '>' => {
                push_word(&mut tokens, &mut word);
                if chars.peek() == Some(&character) {
                    chars.next();
                }
                tokens.push(Token::Operator(Operator::Redirect));
            }
            _ => word.push(character),
        }
    }
    push_word(&mut tokens, &mut word);
    tokens
}

fn push_word(tokens: &mut Vec<Token>, word: &mut String) {
    if !word.is_empty() {
        tokens.push(Token::Word(std::mem::take(word)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_quoted_command_as_data() {
        assert_eq!(
            parse_command_line("echo 'rm -rf /'"),
            vec![CommandInvocation {
                executable: "echo".into(),
                arguments: vec!["rm -rf /".into()],
            }]
        );
    }

    #[test]
    fn parses_nested_shell_and_command_boundaries() {
        let parsed = parse_command_line("printf ok && bash -c 'git reset --hard'");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[2].executable, "git");
        assert_eq!(parsed[2].arguments, ["reset", "--hard"]);
    }

    #[test]
    fn parses_shell_option_clusters_sudo_options_and_substitutions() {
        let parsed = parse_command_line(
            "bash -lc 'git reset --hard'; sudo -u root rm -rf /tmp/data; echo $(find . -delete)",
        );
        assert!(
            parsed
                .iter()
                .any(|call| { call.executable == "git" && call.arguments == ["reset", "--hard"] })
        );
        assert!(
            parsed
                .iter()
                .any(|call| { call.executable == "rm" && call.arguments == ["-rf", "/tmp/data"] })
        );
        assert!(
            parsed
                .iter()
                .any(|call| { call.executable == "find" && call.arguments == [".", "-delete"] })
        );
    }
    #[test]
    fn parses_legacy_and_process_substitutions() {
        let parsed = parse_command_line(
            "printf '%s' `git reset --hard`; diff <(printf safe) <(rm -rf /tmp/work)",
        );
        assert!(parsed.iter().any(|call| call.executable == "git"));
        assert!(parsed.iter().any(|call| call.executable == "rm"));
    }

    #[test]
    fn quoted_parentheses_do_not_hide_commands_in_substitutions() {
        let parsed = parse_command_line(r#"echo "$(printf '('; rm -rf /tmp/work)""#);
        assert!(
            parsed
                .iter()
                .any(|call| { call.executable == "rm" && call.arguments == ["-rf", "/tmp/work"] })
        );
    }

    #[test]
    fn ignores_command_substitution_inside_single_quotes() {
        let parsed = parse_command_line("printf '%s' '$(rm -rf /)'");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].executable, "printf");
    }
}
