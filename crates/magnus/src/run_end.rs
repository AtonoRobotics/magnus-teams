//! What a run held only while it ran, let go when it ends: its browsers, its
//! sandbox (with every file it made or imported there) and the bytes of the
//! attachments it fetched.
use crate::kernel::Kernel;

impl Kernel {
    pub(crate) async fn end_run(&self, run: &str) {
        self.close_run_browsers(run).await;
        self.sandboxes.destroy(run).await;
        crate::attachment_hold::release(run);
    }
}
