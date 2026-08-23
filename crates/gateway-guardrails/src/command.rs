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

    let nested = invocations
        .iter()
        .filter_map(nested_shell_command)
        .flat_map(parse_command_line)
        .collect::<Vec<_>>();
    invocations.extend(nested);
    invocations
}

fn push_invocation(invocations: &mut Vec<CommandInvocation>, words: &mut Vec<String>) {
    if words.is_empty() {
        return;
    }

    let executable_index = words
        .iter()
        .position(|word| !is_assignment(word) && !is_shell_prefix(word));
    if let Some(index) = executable_index {
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
        .position(|argument| argument == "-c" || argument == "--command")
        .and_then(|index| invocation.arguments.get(index + 1))
        .map(String::as_str)
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

fn is_shell_prefix(word: &str) -> bool {
    matches!(
        word,
        "command" | "builtin" | "exec" | "env" | "sudo" | "nohup"
    )
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
}
