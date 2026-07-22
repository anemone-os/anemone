use serde::Deserialize;

use super::{
    build_preset::CargoProfile,
    reference::{BuildPresetRef, KernelConfigRef, SystemTargetRef},
};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SelectionFile {
    pub preset: BuildPresetRef,
}

impl SelectionFile {
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(content)?)
    }
}

pub struct SelectionRequest {
    preset: Option<BuildPresetRef>,
    target: Option<SystemTargetRef>,
    kernel_config: Option<KernelConfigRef>,
    profile: Option<CargoProfile>,
}

impl SelectionRequest {
    pub fn new(
        preset: Option<BuildPresetRef>,
        target: Option<SystemTargetRef>,
        kernel_config: Option<KernelConfigRef>,
        profile: Option<CargoProfile>,
    ) -> Self {
        Self {
            preset,
            target,
            kernel_config,
            profile,
        }
    }

    pub fn implicit() -> Self {
        Self::new(None, None, None, None)
    }

    pub fn explicit_preset(preset: BuildPresetRef) -> Self {
        Self::new(Some(preset), None, None, None)
    }

    pub fn explicit_tuple(
        target: SystemTargetRef,
        kernel_config: KernelConfigRef,
        profile: CargoProfile,
    ) -> Self {
        Self::new(None, Some(target), Some(kernel_config), Some(profile))
    }

    pub(super) fn classify(self) -> anyhow::Result<SelectionChoice> {
        let tuple_fields = usize::from(self.target.is_some())
            + usize::from(self.kernel_config.is_some())
            + usize::from(self.profile.is_some());
        if let Some(preset) = self.preset {
            if tuple_fields != 0 {
                anyhow::bail!("explicit preset and low-level selection are mutually exclusive");
            }
            return Ok(SelectionChoice::Preset(preset));
        }

        match (self.target, self.kernel_config, self.profile) {
            (None, None, None) => Ok(SelectionChoice::Implicit),
            (Some(target), Some(kernel_config), Some(profile)) => Ok(SelectionChoice::Tuple {
                target,
                kernel_config,
                profile,
            }),
            _ => anyhow::bail!(
                "low-level selection requires target, kernel-config, and profile together"
            ),
        }
    }
}

pub(super) enum SelectionChoice {
    Preset(BuildPresetRef),
    Tuple {
        target: SystemTargetRef,
        kernel_config: KernelConfigRef,
        profile: CargoProfile,
    },
    Implicit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_file_is_closed() {
        let selection =
            SelectionFile::from_str("preset = \"qemu-virt-rv64-pretest-release\"\n").unwrap();
        assert_eq!(selection.preset.as_str(), "qemu-virt-rv64-pretest-release");
        assert!(
            SelectionFile::from_str(
                "preset = \"qemu-virt-rv64-pretest-release\"\ntarget = \"other\"\n"
            )
            .is_err()
        );
    }

    #[test]
    fn explicit_sources_are_complete_and_mutually_exclusive() {
        let preset = BuildPresetRef::new("preset").unwrap();
        let target = SystemTargetRef::new("target").unwrap();
        let kernel_config = KernelConfigRef::new("conf/.defconfig").unwrap();

        assert!(matches!(
            SelectionRequest::explicit_preset(preset.clone())
                .classify()
                .unwrap(),
            SelectionChoice::Preset(_)
        ));
        assert!(matches!(
            SelectionRequest::explicit_tuple(
                target.clone(),
                kernel_config.clone(),
                CargoProfile::Release,
            )
            .classify()
            .unwrap(),
            SelectionChoice::Tuple { .. }
        ));
        assert!(matches!(
            SelectionRequest::implicit().classify().unwrap(),
            SelectionChoice::Implicit
        ));

        assert!(
            SelectionRequest::new(
                Some(preset),
                Some(target.clone()),
                Some(kernel_config.clone()),
                Some(CargoProfile::Release),
            )
            .classify()
            .is_err()
        );
        for incomplete in [
            SelectionRequest::new(None, Some(target.clone()), None, None),
            SelectionRequest::new(
                None,
                Some(target.clone()),
                Some(kernel_config.clone()),
                None,
            ),
            SelectionRequest::new(
                None,
                None,
                Some(kernel_config.clone()),
                Some(CargoProfile::Dev),
            ),
        ] {
            assert!(incomplete.classify().is_err());
        }
    }
}
