use crate::{MatchedRule, command::CommandInvocation};

use super::{has_option, option_value, positional_arguments, rule};
pub(super) fn match_shell(invocation: &CommandInvocation) -> Option<MatchedRule> {
    match invocation.executable.as_str() {
        "shutdown" | "reboot" | "poweroff" | "halt" => Some(rule(
            "core.shell",
            "system-power-state",
            "command.executable",
            "shell.system_power_state",
            "Changes the host power state",
            "Use an operator-approved maintenance procedure",
        )),
        "systemctl" if systemctl_changes_power_state(&invocation.arguments) => Some(rule(
            "core.shell",
            "systemctl-power-state",
            "command.arguments",
            "shell.systemctl_power_state",
            "Changes the host power state through systemd",
            "Use an operator-approved maintenance procedure",
        )),
        "init" | "telinit" if targets_shutdown_runlevel(&invocation.arguments) => Some(rule(
            "core.shell",
            "init-power-state",
            "command.arguments",
            "shell.init_power_state",
            "Changes the host power state through an init runlevel",
            "Use an operator-approved maintenance procedure",
        )),
        executable if executable.starts_with("mkfs") => Some(rule(
            "core.shell",
            "format-filesystem",
            "command.executable",
            "shell.format_filesystem",
            "Formats a block device",
            "Verify the target device and use a provisioned replacement volume",
        )),
        "kill" if targets_pid_one(&invocation.arguments) => Some(rule(
            "core.shell",
            "kill-init",
            "command.arguments",
            "shell.kill_init",
            "Terminates the host init process",
            "Restart the intended service through its service manager",
        )),
        "kill" if targets_broadcast_processes(&invocation.arguments) => Some(rule(
            "core.shell",
            "kill-broadcast",
            "command.arguments",
            "shell.kill_broadcast",
            "Sends a signal to every permitted process or a process group",
            "Target one reviewed process ID through its service manager",
        )),
        executable
            if matches!(executable, "sh" | "bash" | "zsh" | "dash" | "fish")
                && crate::command::nested_shell_command(invocation).is_none() =>
        {
            Some(rule(
                "core.shell",
                "uninspectable-shell-input",
                "command.arguments",
                "shell.uninspectable_input",
                "Runs shell input that the policy cannot inspect",
                "Pass the command through the shell command option",
            ))
        }
        executable if executable.starts_with('$') => Some(rule(
            "core.shell",
            "dynamic-executable",
            "command.executable",
            "shell.dynamic_executable",
            "Resolves the executable from an unknown shell variable",
            "Use an explicit executable name that policy can inspect",
        )),
        _ => None,
    }
}

fn systemctl_changes_power_state(arguments: &[String]) -> bool {
    let positional = positional_arguments(arguments);
    positional.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "poweroff" | "reboot" | "halt" | "kexec" | "soft-reboot"
        )
    }) || positional.windows(2).any(|window| {
        window[0] == "start"
            && matches!(
                window[1].as_str(),
                "poweroff.target" | "reboot.target" | "halt.target"
            )
    })
}

fn targets_shutdown_runlevel(arguments: &[String]) -> bool {
    arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "0" | "6"))
}

fn targets_pid_one(arguments: &[String]) -> bool {
    arguments
        .iter()
        .filter(|argument| !argument.starts_with('-'))
        .any(|argument| argument == "1")
}

fn targets_broadcast_processes(arguments: &[String]) -> bool {
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--" {
            return arguments[index + 1..].iter().any(|target| target == "-1");
        }
        if matches!(argument, "-s" | "--signal" | "-n") {
            index += 2;
            continue;
        }
        if argument.starts_with("--signal=") {
            index += 1;
            continue;
        }
        if index == 0 && argument.starts_with('-') {
            index += 1;
            continue;
        }
        if argument == "-1" {
            return true;
        }
        index += 1;
    }
    false
}

pub(super) fn match_filesystem(invocation: &CommandInvocation) -> Option<MatchedRule> {
    let arguments = &invocation.arguments;
    match invocation.executable.as_str() {
        "rm" if (has_option(arguments, "--recursive", Some('r'))
            || has_option(arguments, "--recursive", Some('R')))
            && has_option(arguments, "--force", Some('f')) =>
        {
            Some(rule(
                "core.filesystem",
                "recursive-force-remove",
                "command.arguments",
                "filesystem.recursive_force_remove",
                "Recursively deletes files without confirmation",
                "List and review the target, then remove explicit paths without force",
            ))
        }
        "rm" if has_option(arguments, "--recursive", Some('r'))
            || has_option(arguments, "--recursive", Some('R')) =>
        {
            Some(rule(
                "core.filesystem",
                "recursive-remove",
                "command.arguments",
                "filesystem.recursive_remove",
                "Recursively deletes a directory tree",
                "List and review the target, then remove explicit paths",
            ))
        }
        "rmdir" | "unlink" => Some(rule(
            "core.filesystem",
            "remove-path",
            "command.executable",
            "filesystem.remove_path",
            "Removes a filesystem path",
            "Verify and preserve the target before removing it",
        )),
        "shred" => Some(rule(
            "core.filesystem",
            "shred-file",
            "command.executable",
            "filesystem.shred_file",
            "Overwrites file contents to prevent recovery",
            "Preserve required data and use a reviewed disposal procedure",
        )),
        "truncate" if truncate_shrinks(arguments) => Some(rule(
            "core.filesystem",
            "truncate-file",
            "command.arguments",
            "filesystem.truncate_file",
            "Shrinks or clears file contents",
            "Preserve the file and write the intended content to a new path",
        )),
        "dd" if arguments
            .iter()
            .any(|argument| argument.starts_with("of=/dev/")) =>
        {
            Some(rule(
                "core.filesystem",
                "overwrite-device",
                "command.arguments",
                "filesystem.overwrite_device",
                "Writes raw data to a block or character device",
                "Verify the device and use an operator-approved imaging procedure",
            ))
        }
        "wipefs"
            if has_option(arguments, "--all", Some('a'))
                || has_option(arguments, "--offset", Some('o')) =>
        {
            Some(rule(
                "core.filesystem",
                "wipe-filesystem-signature",
                "command.arguments",
                "filesystem.wipe_signature",
                "Erases filesystem, RAID, or partition-table signatures",
                "Inventory the device and preserve metadata before erasing signatures",
            ))
        }
        "diskutil" if diskutil_erases_storage(arguments) => Some(rule(
            "core.filesystem",
            "diskutil-erase",
            "command.arguments",
            "filesystem.diskutil_erase",
            "Erases, repartitions, or deletes a macOS storage volume",
            "Verify the disk identifier and preserve required data first",
        )),
        "find" if arguments.iter().any(|argument| argument == "-delete") => Some(rule(
            "core.filesystem",
            "find-delete",
            "command.arguments",
            "filesystem.find_delete",
            "Deletes every path selected by find",
            "Run find without -delete and review the selected paths",
        )),
        _ => None,
    }
}

fn truncate_shrinks(arguments: &[String]) -> bool {
    option_value(arguments, &["-s", "--size"]).is_some_and(|size| {
        let (negative, magnitude) = size
            .strip_prefix('-')
            .map_or((false, size), |value| (true, value));
        let digit_count = magnitude.bytes().take_while(u8::is_ascii_digit).count();
        if digit_count == 0
            || !magnitude[digit_count..]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic())
        {
            return false;
        }
        negative || magnitude[..digit_count].bytes().all(|byte| byte == b'0')
    })
}

fn diskutil_erases_storage(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.to_ascii_lowercase().as_str(),
            "erasedisk" | "erasevolume" | "partitiondisk" | "deletevolume"
        )
    })
}
