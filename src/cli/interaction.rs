use std::io::{BufRead, Write, stdin, stdout};

use eyre::{WrapErr, eyre};
use owo_colors::OwoColorize;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PromptChoice {
    Yes,
    No,
    Explain,
}

pub(crate) fn prompt(
    question: impl AsRef<str>,
    default: PromptChoice,
    currently_explaining: bool,
) -> eyre::Result<PromptChoice> {
    let mut stdout = stdout();
    let with_confirm = format!(
        "\
        {question}\n\
        \n\
        {are_you_sure} ({yes}/{no}{maybe_explain}): \
    ",
        question = question.as_ref(),
        are_you_sure = "Proceed?".bold(),
        no = if default == PromptChoice::No {
            "[N]o"
        } else {
            "[n]o"
        }
        .red(),
        yes = if default == PromptChoice::Yes {
            "[Y]es"
        } else {
            "[y]es"
        }
        .green(),
        maybe_explain = if !currently_explaining {
            format!(
                "/{}",
                if default == PromptChoice::Explain {
                    "[E]xplain"
                } else {
                    "[e]xplain"
                }
            )
        } else {
            "".into()
        },
    );

    stdout.write_all(with_confirm.as_bytes())?;
    stdout.flush()?;

    let input = read_line()?;

    let r = match &*input.to_lowercase() {
        "y" | "yes" => PromptChoice::Yes,
        "n" | "no" => PromptChoice::No,
        "e" | "explain" => PromptChoice::Explain,
        "" => default,
        _ => PromptChoice::No,
    };

    Ok(r)
}

pub(crate) fn read_line() -> eyre::Result<String> {
    let stdin = stdin();
    let stdin = stdin.lock();
    let mut lines = stdin.lines();
    let lines = lines.next().transpose()?;
    match lines {
        None => Err(eyre!("no lines found from stdin")),
        Some(v) => Ok(v),
    }
    .context("unable to read from stdin for confirmation")
}

pub(crate) fn clean_exit_with_message(message: impl AsRef<str>) -> ! {
    eprintln!("{}", message.as_ref());
    std::process::exit(0)
}
