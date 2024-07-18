// This help page was automatically generated from the mangen.py tool.
pub const HELP: &str = r#"Plan and execute a target.

Usage:
    orbit build [options] [--] [args]...

Options:
    --target <name>         target to execute
    --top <unit>            set the top level design unit
    --plan <format>         set the blueprint file format
    --target-dir <dir>      the relative directory where the target starts
    --command <path>        overwrite the target's command
    --list                  view available targets and exit
    --all                   include all hdl files of the working ip
    --fileset <key=glob>... set filesets for the target
    --force                 force the target to execute
    --no-clean              do not clean the target folder before execution
    --verbose               display the command being executed
    args                    arguments to pass to the requested command

Use 'orbit help build' to read more about the command.
"#;
