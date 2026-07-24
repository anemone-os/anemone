use crate::{
    arch::loongarch64::machine::{MachineDesc, MachineIpi},
    device::discovery::open_firmware::{get_of_node, of_with_node_by_path},
    driver::intc::loongson_platic::LA7A1000Platic,
    prelude::*,
    utils::identity::GeneralIdentity,
};
use loongArch64::{
    iocsr::iocsr_write_w,
    ipi::{csr_mail_send, send_ipi_single},
};

/// QEMU virt machine description for the Loongson 3A5000-compatible board.
#[derive(Debug)]
pub struct Qemu3A5000;

impl MachineDesc for Qemu3A5000 {
    /// Device-tree compatible string used by QEMU virt.
    fn compatible(&self) -> &[&str] {
        &["linux,dummy-loongson3"]
    }

    fn ipi(&self) -> &dyn MachineIpi {
        self
    }

    /// Bring up the platform interrupt controller discovered from firmware.
    unsafe fn early_init_intc(&self) {
        kinfoln!("initializing interrupt controller for qemu virt machine");

        let plic = {
            match of_with_node_by_path("/platic", |node| {
                kdebugln!("found platic node: {}", node.path());
                node.handle()
            }) {
                Ok(node) => get_of_node(node),
                Err(_) => panic!("failed to find platic node in device tree"),
            }
        };
        plic.mark_populated();

        let plic_ops = LA7A1000Platic::init(plic.as_ref());

        unsafe {
            register_root_irq_domain(
                GeneralIdentity::try_from(plic.node().full_name()).unwrap(),
                plic_ops,
                plic,
            );
        }
    }

    /// Initialize the machine timer through the common time architecture hook.
    unsafe fn early_init_timer(&self) {
        // no-op; we may extend machine init to support percpu?
    }
}

impl MachineIpi for Qemu3A5000 {
    fn send_ipi(&self, target: PhysCpuId) {
        send_ipi_single(target.get(), 1);
    }

    unsafe fn claim_ipi(&self) {
        iocsr_write_w(0x100c, u32::MAX);
    }

    unsafe fn init_local_ipi(&self) {
        iocsr_write_w(0x1004, u32::MAX);
    }

    fn wake_secondary(&self, target: PhysCpuId, entry: PhysAddr) {
        csr_mail_send(entry.get(), target.get(), 0);
        self.send_ipi(target);
    }
}
