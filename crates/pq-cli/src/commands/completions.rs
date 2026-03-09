use clap::CommandFactory;
use clap_complete::generate;

pub fn run(shell: clap_complete::Shell) -> anyhow::Result<()> {
    let mut cmd = crate::cli::Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}
