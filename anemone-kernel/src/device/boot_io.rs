//! Boot-time console and TTY composition.

use crate::{
    device::{console, tty},
    prelude::*,
};

/// The three pre-opened terminal files transferred exactly once to init.
pub(crate) struct InitStdio([File; 3]);

impl InitStdio {
    pub(crate) fn into_files(self) -> [File; 3] {
        self.0
    }
}

/// Prepare both owner-local publication transactions before making either
/// namespace visible, then commit them in the fixed console -> TTY order.
/// A failure after the first publish is boot-fatal at the caller; runtime
/// rollback is deliberately outside the boot endpoint protocol.
pub(crate) fn finalize(selection: &console::ConsoleSelection) -> Result<InitStdio, SysError> {
    let console_publication = console::prepare_devfs()?;
    let tty_publication = tty::prepare_system_boot(selection.terminal_identity())?;

    console_publication.publish()?;
    let files = tty_publication.publish()?;
    Ok(InitStdio(files))
}
