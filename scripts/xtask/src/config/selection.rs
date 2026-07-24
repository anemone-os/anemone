use super::{
    build_preset::CargoProfile,
    reference::{BuildPresetRef, KernelConfigRef, SystemTargetRef, validate_slug},
};
use clap::Args;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

pub type BindValues = HashMap<String, String>;

#[derive(Args, Debug, Default)]
pub struct BindArgs {
    #[arg(long = "bind", value_name = "NAME=VALUE")]
    #[arg(help = "Bind an opaque value referenced by the selected QEMU Platform")]
    bind: Vec<BindValue>,
}

impl BindArgs {
    pub fn into_values(self) -> anyhow::Result<BindValues> {
        let mut values = HashMap::new();
        for binding in self.bind {
            validate_slug("bind", &binding.name)?;
            if binding.value.is_empty() {
                anyhow::bail!("bind `{}` value must not be empty", binding.name);
            }
            if values.insert(binding.name.clone(), binding.value).is_some() {
                anyhow::bail!("duplicate bind value `{}`", binding.name);
            }
        }
        Ok(values)
    }
}

pub fn reject_unconsumed_bindings(
    values: &BindValues,
    consumed: &HashSet<String>,
) -> anyhow::Result<()> {
    if let Some(name) = values.keys().find(|name| !consumed.contains(*name)) {
        anyhow::bail!("unknown or unconsumed bind `{name}`");
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct BindValue {
    name: String,
    value: String,
}

impl FromStr for BindValue {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (name, value) = input
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("bind must use NAME=VALUE"))?;
        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }
}

pub struct SelectionRequest {
    preset: Option<BuildPresetRef>,
    target: Option<SystemTargetRef>,
    kernel_config: Option<KernelConfigRef>,
    profile: Option<CargoProfile>,
}

#[derive(Args, Debug)]
pub struct SelectionArgs {
    #[arg(long, value_name = "PRESET")]
    #[arg(help = "Select a tracked build preset")]
    preset: Option<String>,

    #[arg(long, value_name = "TARGET")]
    #[arg(help = "Select a system target as part of a complete low-level tuple")]
    target: Option<String>,

    #[arg(long, value_name = "PATH")]
    #[arg(help = "Select a KernelConfig as part of a complete low-level tuple")]
    kernel_config: Option<String>,

    #[arg(long, value_name = "PROFILE")]
    #[arg(help = "Select the kernel Cargo profile as part of a complete low-level tuple")]
    profile: Option<CargoProfile>,
}

impl SelectionArgs {
    pub fn into_request(self) -> anyhow::Result<SelectionRequest> {
        Ok(SelectionRequest::new(
            self.preset
                .as_deref()
                .map(BuildPresetRef::new)
                .transpose()?,
            self.target
                .as_deref()
                .map(SystemTargetRef::new)
                .transpose()?,
            self.kernel_config
                .as_deref()
                .map(KernelConfigRef::new)
                .transpose()?,
            self.profile,
        ))
    }
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
            (None, None, None) => anyhow::bail!(
                "system action requires either --preset or --target, --kernel-config, and --profile together"
            ),
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
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct SelectionCli {
        #[command(flatten)]
        selection: SelectionArgs,
    }

    #[derive(Parser)]
    struct BindCli {
        #[command(flatten)]
        bindings: BindArgs,
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
        assert!(
            SelectionRequest::new(None, None, None, None)
                .classify()
                .is_err()
        );

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

    #[test]
    fn bindings_are_opaque_nonempty_and_unique() {
        let bindings = BindCli::try_parse_from([
            "test",
            "--bind",
            "memory=8G",
            "--bind",
            "disk-x1=path,with={{literal}}",
        ])
        .unwrap()
        .bindings
        .into_values()
        .unwrap();
        assert_eq!(bindings["memory"], "8G");
        assert_eq!(bindings["disk-x1"], "path,with={{literal}}");

        for arguments in [
            vec!["test", "--bind", "memory="],
            vec!["test", "--bind", "memory=1G", "--bind", "memory=8G"],
            vec!["test", "--bind", "not_valid=1"],
        ] {
            let parsed = BindCli::try_parse_from(arguments).unwrap();
            assert!(parsed.bindings.into_values().is_err());
        }

        assert!(reject_unconsumed_bindings(&bindings, &HashSet::new()).is_err());
        assert!(
            reject_unconsumed_bindings(
                &bindings,
                &HashSet::from(["memory".to_string(), "disk-x1".to_string()]),
            )
            .is_ok()
        );
    }

    #[test]
    fn clap_selection_args_feed_the_same_classifier() {
        let preset = SelectionCli::try_parse_from(["test", "--preset", "test-release"])
            .unwrap()
            .selection
            .into_request()
            .unwrap();
        assert!(matches!(
            preset.classify().unwrap(),
            SelectionChoice::Preset(_)
        ));

        let tuple = SelectionCli::try_parse_from([
            "test",
            "--target",
            "example",
            "--kernel-config",
            "conf/.defconfig",
            "--profile",
            "dev",
        ])
        .unwrap()
        .selection
        .into_request()
        .unwrap();
        assert!(matches!(
            tuple.classify().unwrap(),
            SelectionChoice::Tuple { .. }
        ));

        assert!(SelectionCli::try_parse_from(["test", "--profile", "unsupported"]).is_err());
    }
}
