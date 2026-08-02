//! Kernel symbol-table extraction, encoding, and final-ELF verification.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    fs,
    ops::Range,
    path::Path,
};

use anyhow::Context;
use goblin::elf::{
    Elf,
    program_header::{PF_R, PF_W, PF_X, PT_LOAD},
    section_header::{SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE},
    sym::{STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FUNC, st_bind, st_type},
};
use symtab::{
    ENTRY_SIZE, HEADER_SIZE, SymbolTable,
    builder::{SymbolInput, encode},
};

pub(super) const GENERATED_TABLE: &str = "build/generated/kernel.symtab";
pub(super) const DISCOVERY_ELF: &str = "build/generated/anemone-discovery.elf";
pub(super) const PASS_MAP: &str = "build/generated/kernel-pass.map";

const TABLE_SECTION: &str = ".anemone.symtab";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KernelSymbol {
    pub start: u64,
    pub size: u64,
    pub name: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    start: u64,
    size: u64,
    raw_name: String,
    binding: u8,
    defined: bool,
    function: bool,
    alloc: bool,
    executable: bool,
}

#[derive(Clone, Debug)]
struct VerificationSnapshot {
    final_symbols: Vec<KernelSymbol>,
    table_bytes: Vec<u8>,
    section_index: usize,
    section_flags: u64,
    section_start: u64,
    section_end: u64,
    etext: u64,
    rodata_start: u64,
    rodata_end: u64,
    linker_start: u64,
    linker_end: u64,
    load_flags: Option<u32>,
    executable_ranges: Vec<Range<u64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TableStats {
    pub entries: usize,
    pub strings: usize,
    pub total: usize,
}

pub(super) fn prepare_empty_table() -> anyhow::Result<()> {
    write_encoded_table(&[]).context("failed to write discovery symbol table")?;
    Ok(())
}

pub(super) fn generate_from_discovery(path: &Path) -> anyhow::Result<Vec<KernelSymbol>> {
    let symbols = extract_symbols(path)?;
    write_encoded_table(&symbols).context("failed to write final kernel symbol table")?;
    Ok(symbols)
}

fn verify_final_elf(discovery: &[KernelSymbol], final_elf: &Path) -> anyhow::Result<TableStats> {
    let bytes = fs::read(final_elf)
        .with_context(|| format!("failed to read final ELF '{}'", final_elf.display()))?;
    verify_final_bytes(discovery, &bytes).with_context(|| {
        format!(
            "final ELF '{}' failed symbol verification",
            final_elf.display()
        )
    })
}

pub(super) fn verify_and_publish(
    discovery: &[KernelSymbol],
    final_elf: &Path,
    final_map: &Path,
    public_elf: &Path,
    public_map: &Path,
) -> anyhow::Result<TableStats> {
    let stats = verify_final_elf(discovery, final_elf)?;
    publish_verified(final_elf, final_map, public_elf, public_map)?;
    Ok(stats)
}

fn publish_verified(
    final_elf: &Path,
    final_map: &Path,
    public_elf: &Path,
    public_map: &Path,
) -> anyhow::Result<()> {
    // The public ELF is the action commit point. Publish its auxiliary map
    // first so a map failure cannot pair a new ELF with a stale map.
    fs::copy(final_map, public_map).with_context(|| {
        format!(
            "failed to publish final map '{}' as '{}'",
            final_map.display(),
            public_map.display()
        )
    })?;
    fs::copy(final_elf, public_elf).with_context(|| {
        format!(
            "failed to publish verified final ELF '{}' as '{}'",
            final_elf.display(),
            public_elf.display()
        )
    })?;
    Ok(())
}

fn write_encoded_table(symbols: &[KernelSymbol]) -> anyhow::Result<TableStats> {
    let inputs: Vec<_> = symbols
        .iter()
        .map(|symbol| SymbolInput::new(symbol.start, symbol.size, symbol.name.as_str()))
        .collect();
    let bytes = encode(&inputs)?;
    let stats = table_stats(&bytes)?;
    fs::write(GENERATED_TABLE, &bytes)
        .with_context(|| format!("failed to write generated table '{GENERATED_TABLE}'"))?;
    Ok(stats)
}

fn table_stats(bytes: &[u8]) -> anyhow::Result<TableStats> {
    let table = SymbolTable::parse(bytes)?;
    let entries = table.len();
    let strings = bytes
        .len()
        .checked_sub(HEADER_SIZE + entries * ENTRY_SIZE)
        .context("validated table size underflow")?;
    Ok(TableStats {
        entries,
        strings,
        total: bytes.len(),
    })
}

fn extract_symbols(path: &Path) -> anyhow::Result<Vec<KernelSymbol>> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read kernel ELF '{}'", path.display()))?;
    let elf = Elf::parse(&bytes)
        .with_context(|| format!("failed to parse kernel ELF '{}'", path.display()))?;
    extract_symbols_from_elf(&elf)
}

fn extract_symbols_from_elf(elf: &Elf<'_>) -> anyhow::Result<Vec<KernelSymbol>> {
    let mut candidates = Vec::new();
    for symbol in elf.syms.iter() {
        let section = elf.section_headers.get(symbol.st_shndx);
        let raw_name = elf.strtab.get_at(symbol.st_name).ok_or_else(|| {
            anyhow::anyhow!(
                "ELF symbol at string offset {} has no valid UTF-8 name",
                symbol.st_name
            )
        })?;
        candidates.push(Candidate {
            start: symbol.st_value,
            size: symbol.st_size,
            raw_name: raw_name.to_owned(),
            binding: st_bind(symbol.st_info),
            defined: symbol.st_shndx != 0 && section.is_some(),
            function: st_type(symbol.st_info) == STT_FUNC,
            alloc: section.is_some_and(|section| section.sh_flags & u64::from(SHF_ALLOC) != 0),
            executable: section
                .is_some_and(|section| section.sh_flags & u64::from(SHF_EXECINSTR) != 0),
        });
    }
    canonicalize(candidates)
}

fn canonicalize(
    candidates: impl IntoIterator<Item = Candidate>,
) -> anyhow::Result<Vec<KernelSymbol>> {
    let mut selected = BTreeMap::<u64, Candidate>::new();
    for candidate in candidates {
        if !candidate.defined
            || !candidate.function
            || !candidate.alloc
            || !candidate.executable
            || candidate.size == 0
            || candidate.raw_name.is_empty()
            || candidate.raw_name.starts_with(".L")
        {
            continue;
        }
        candidate.start.checked_add(candidate.size).ok_or_else(|| {
            anyhow::anyhow!(
                "ELF symbol '{}' range overflows: start={:#x} size={:#x}",
                candidate.raw_name,
                candidate.start,
                candidate.size
            )
        })?;
        match selected.entry(candidate.start) {
            Entry::Vacant(entry) => {
                entry.insert(candidate);
            },
            Entry::Occupied(mut entry) => {
                if preferred(&candidate, entry.get()) {
                    entry.insert(candidate);
                }
            },
        }
    }

    selected
        .into_values()
        .map(|candidate| {
            let name = match rustc_demangle::try_demangle(&candidate.raw_name) {
                Ok(demangled) => format!("{demangled:#}"),
                Err(_) => candidate.raw_name,
            };
            if name.is_empty() {
                anyhow::bail!(
                    "ELF symbol at {:#x} has an empty display name",
                    candidate.start
                );
            }
            Ok(KernelSymbol {
                start: candidate.start,
                size: candidate.size,
                name,
            })
        })
        .collect()
}

fn preferred(candidate: &Candidate, current: &Candidate) -> bool {
    candidate.size > current.size
        || (candidate.size == current.size
            && (binding_rank(candidate.binding) > binding_rank(current.binding)
                || (binding_rank(candidate.binding) == binding_rank(current.binding)
                    && candidate.raw_name.as_bytes() < current.raw_name.as_bytes())))
}

fn binding_rank(binding: u8) -> u8 {
    match binding {
        STB_GLOBAL => 3,
        STB_WEAK => 2,
        STB_LOCAL => 1,
        _ => 0,
    }
}

fn verify_final_bytes(discovery: &[KernelSymbol], bytes: &[u8]) -> anyhow::Result<TableStats> {
    let elf = Elf::parse(bytes).context("failed to parse final kernel ELF")?;
    let final_symbols = extract_symbols_from_elf(&elf)?;
    let (section_index, section) = elf
        .section_headers
        .iter()
        .enumerate()
        .find(|(_, section)| elf.shdr_strtab.get_at(section.sh_name) == Some(TABLE_SECTION))
        .context("final ELF has no .anemone.symtab section")?;
    let file_start =
        usize::try_from(section.sh_offset).context("symbol section offset overflow")?;
    let file_size = usize::try_from(section.sh_size).context("symbol section size overflow")?;
    let file_end = file_start
        .checked_add(file_size)
        .context("symbol section file range overflow")?;
    let table_bytes = bytes
        .get(file_start..file_end)
        .context("symbol section extends beyond final ELF")?
        .to_vec();

    let section_end = section
        .sh_addr
        .checked_add(section.sh_size)
        .context("symbol section address range overflow")?;
    let etext = linker_symbol(&elf, "__etext")?;
    let rodata_start = linker_symbol(&elf, "__srodata")?;
    let rodata_end = linker_symbol(&elf, "__erodata")?;
    let table_start = linker_symbol(&elf, "__sanemone_symtab")?;
    let table_end = linker_symbol(&elf, "__eanemone_symtab")?;
    let load_flags = elf
        .program_headers
        .iter()
        .find(|segment| {
            segment.p_type == PT_LOAD
                && section.sh_addr >= segment.p_vaddr
                && section_end <= segment.p_vaddr.saturating_add(segment.p_memsz)
        })
        .map(|segment| segment.p_flags);

    let executable_ranges: Vec<_> = elf
        .section_headers
        .iter()
        .filter(|section| {
            section.sh_flags & u64::from(SHF_ALLOC | SHF_EXECINSTR)
                == u64::from(SHF_ALLOC | SHF_EXECINSTR)
        })
        .filter_map(|section| {
            section
                .sh_addr
                .checked_add(section.sh_size)
                .map(|end| section.sh_addr..end)
        })
        .collect();

    verify_snapshot(
        discovery,
        &VerificationSnapshot {
            final_symbols,
            table_bytes,
            section_index,
            section_flags: section.sh_flags,
            section_start: section.sh_addr,
            section_end,
            etext,
            rodata_start,
            rodata_end,
            linker_start: table_start,
            linker_end: table_end,
            load_flags,
            executable_ranges,
        },
    )
}

fn verify_snapshot(
    discovery: &[KernelSymbol],
    snapshot: &VerificationSnapshot,
) -> anyhow::Result<TableStats> {
    if snapshot.final_symbols != discovery {
        anyhow::bail!(
            "discovery/final text symbol mapping drifted (discovery={}, final={})",
            discovery.len(),
            snapshot.final_symbols.len()
        );
    }
    if snapshot.section_flags & u64::from(SHF_ALLOC) == 0
        || snapshot.section_flags & u64::from(SHF_WRITE | SHF_EXECINSTR) != 0
    {
        anyhow::bail!(
            ".anemone.symtab section {} must be allocatable, read-only, and non-executable (flags={:#x})",
            snapshot.section_index,
            snapshot.section_flags
        );
    }
    if snapshot.section_start < snapshot.etext {
        anyhow::bail!(
            ".anemone.symtab starts before __etext ({:#x} < {:#x})",
            snapshot.section_start,
            snapshot.etext
        );
    }
    if snapshot.section_start < snapshot.rodata_start || snapshot.section_end > snapshot.rodata_end
    {
        anyhow::bail!(
            ".anemone.symtab range {:#x}..{:#x} is outside the runtime rodata mapping {:#x}..{:#x}",
            snapshot.section_start,
            snapshot.section_end,
            snapshot.rodata_start,
            snapshot.rodata_end
        );
    }
    if snapshot.linker_start != snapshot.section_start
        || snapshot.linker_end != snapshot.section_end
    {
        anyhow::bail!(
            "linker symbol table range {:#x}..{:#x} does not match section {:#x}..{:#x}",
            snapshot.linker_start,
            snapshot.linker_end,
            snapshot.section_start,
            snapshot.section_end
        );
    }
    let load_flags = snapshot
        .load_flags
        .context(".anemone.symtab is not covered by a loadable segment")?;
    if load_flags & PF_R == 0 || load_flags & (PF_W | PF_X) != 0 {
        anyhow::bail!(
            ".anemone.symtab load segment must be readable, non-writable, and non-executable (flags={load_flags:#x})"
        );
    }

    let table =
        SymbolTable::parse(&snapshot.table_bytes).context("embedded symbol table is malformed")?;
    let embedded: Vec<_> = table
        .iter()
        .map(|symbol| KernelSymbol {
            start: symbol.start,
            size: symbol.size,
            name: symbol.name.to_owned(),
        })
        .collect();
    if embedded != snapshot.final_symbols {
        anyhow::bail!(
            "embedded table does not match final ELF symbols (embedded={}, final={})",
            embedded.len(),
            snapshot.final_symbols.len()
        );
    }
    for symbol in table.iter() {
        let end = symbol
            .start
            .checked_add(symbol.size)
            .context("validated symbol range overflow")?;
        if !snapshot
            .executable_ranges
            .iter()
            .any(|range| range.start <= symbol.start && end <= range.end)
        {
            anyhow::bail!(
                "embedded symbol '{}' range {:#x}..{end:#x} is outside executable sections",
                symbol.name,
                symbol.start
            );
        }
    }

    table_stats(&snapshot.table_bytes)
}

fn linker_symbol(elf: &Elf<'_>, name: &str) -> anyhow::Result<u64> {
    elf.syms
        .iter()
        .find(|symbol| elf.strtab.get_at(symbol.st_name) == Some(name))
        .map(|symbol| symbol.st_value)
        .with_context(|| format!("final ELF has no linker symbol '{name}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn candidate(start: u64, size: u64, name: &str, binding: u8) -> Candidate {
        Candidate {
            start,
            size,
            raw_name: name.to_owned(),
            binding,
            defined: true,
            function: true,
            alloc: true,
            executable: true,
        }
    }

    #[test]
    fn selection_filters_non_functions_and_canonicalizes_aliases() {
        let mut undefined = candidate(1, 1, "undefined", STB_GLOBAL);
        undefined.defined = false;
        let mut non_function = candidate(2, 1, "object", STB_GLOBAL);
        non_function.function = false;
        let mut non_exec = candidate(3, 1, "rodata", STB_GLOBAL);
        non_exec.executable = false;
        let symbols = canonicalize([
            undefined,
            non_function,
            non_exec,
            candidate(4, 1, ".Lcompiler", STB_LOCAL),
            candidate(5, 2, "weak_large", STB_WEAK),
            candidate(5, 1, "global_small", STB_GLOBAL),
            candidate(6, 2, "weak_same", STB_WEAK),
            candidate(6, 2, "global_same", STB_GLOBAL),
            candidate(7, 2, "zeta", STB_GLOBAL),
            candidate(7, 2, "alpha", STB_GLOBAL),
        ])
        .unwrap();
        assert_eq!(
            symbols,
            vec![
                KernelSymbol {
                    start: 5,
                    size: 2,
                    name: "weak_large".to_owned(),
                },
                KernelSymbol {
                    start: 6,
                    size: 2,
                    name: "global_same".to_owned(),
                },
                KernelSymbol {
                    start: 7,
                    size: 2,
                    name: "alpha".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn rust_names_are_demangled_before_encoding() {
        let symbols =
            canonicalize([candidate(1, 1, "_RNvCs3kAxCWUtVg_7mycrate3foo", STB_GLOBAL)]).unwrap();
        assert_eq!(symbols[0].name, "mycrate::foo");
    }

    #[test]
    fn invalid_utf8_elf_string_table_is_rejected_before_selection() {
        let malformed = [0, 0xff, 0];
        assert!(goblin::strtab::Strtab::parse(&malformed, 0, malformed.len(), 0).is_err());
    }

    fn kernel_symbol(name: &str) -> KernelSymbol {
        KernelSymbol {
            start: 0x1000,
            size: 0x10,
            name: name.to_owned(),
        }
    }

    fn snapshot() -> VerificationSnapshot {
        let symbol = kernel_symbol("known");
        let table_bytes = encode(&[SymbolInput::new(
            symbol.start,
            symbol.size,
            symbol.name.as_str(),
        )])
        .unwrap();
        let section_start = 0x2000;
        VerificationSnapshot {
            final_symbols: vec![symbol],
            section_index: 3,
            section_flags: u64::from(SHF_ALLOC),
            section_start,
            section_end: section_start + table_bytes.len() as u64,
            etext: section_start,
            rodata_start: section_start,
            rodata_end: section_start + table_bytes.len() as u64,
            linker_start: section_start,
            linker_end: section_start + table_bytes.len() as u64,
            load_flags: Some(PF_R),
            executable_ranges: vec![0x1000..0x1010],
            table_bytes,
        }
    }

    #[test]
    fn final_verification_rejects_every_protected_boundary_drift() {
        let valid = snapshot();
        let discovery = valid.final_symbols.clone();
        assert!(verify_snapshot(&discovery, &valid).is_ok());

        let mut drifted = valid.clone();
        drifted.final_symbols[0].size += 1;
        assert!(verify_snapshot(&discovery, &drifted).is_err());

        let mut malformed = valid.clone();
        malformed.table_bytes[0] ^= 1;
        assert!(verify_snapshot(&discovery, &malformed).is_err());

        let mut mismatched = valid.clone();
        mismatched.table_bytes = encode(&[SymbolInput::new(0x1000, 0x10, "other")]).unwrap();
        assert!(verify_snapshot(&discovery, &mismatched).is_err());

        for flags in [0, SHF_ALLOC | SHF_WRITE, SHF_ALLOC | SHF_EXECINSTR] {
            let mut bad_flags = valid.clone();
            bad_flags.section_flags = u64::from(flags);
            assert!(verify_snapshot(&discovery, &bad_flags).is_err());
        }

        let mut before_text_end = valid.clone();
        before_text_end.etext += 1;
        assert!(verify_snapshot(&discovery, &before_text_end).is_err());

        let mut outside_rodata = valid.clone();
        outside_rodata.rodata_start += 1;
        assert!(verify_snapshot(&discovery, &outside_rodata).is_err());

        let mut past_rodata = valid.clone();
        past_rodata.rodata_end -= 1;
        assert!(verify_snapshot(&discovery, &past_rodata).is_err());

        let mut bad_linker_range = valid.clone();
        bad_linker_range.linker_end += 1;
        assert!(verify_snapshot(&discovery, &bad_linker_range).is_err());

        let mut not_loadable = valid.clone();
        not_loadable.load_flags = None;
        assert!(verify_snapshot(&discovery, &not_loadable).is_err());

        for flags in [0, PF_R | PF_W, PF_R | PF_X] {
            let mut bad_load = valid.clone();
            bad_load.load_flags = Some(flags);
            assert!(verify_snapshot(&discovery, &bad_load).is_err());
        }

        let mut outside_text = valid;
        outside_text.executable_ranges.clear();
        assert!(verify_snapshot(&discovery, &outside_text).is_err());
    }

    #[test]
    fn verification_failure_cannot_replace_public_artifacts() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "anemone-xtask-symtab-publish-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let final_elf = root.join("final.elf");
        let final_map = root.join("final.map");
        let public_elf = root.join("anemone.elf");
        let public_map = root.join("kernel.map");
        fs::write(&final_elf, b"not an ELF").unwrap();
        fs::write(&final_map, b"new map").unwrap();
        fs::write(&public_elf, b"old ELF").unwrap();
        fs::write(&public_map, b"old map").unwrap();

        assert!(verify_and_publish(&[], &final_elf, &final_map, &public_elf, &public_map).is_err());
        assert_eq!(fs::read(&public_elf).unwrap(), b"old ELF");
        assert_eq!(fs::read(&public_map).unwrap(), b"old map");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn map_publication_failure_cannot_commit_the_new_elf() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "anemone-xtask-symtab-map-publish-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let final_elf = root.join("final.elf");
        let missing_final_map = root.join("missing-final.map");
        let public_elf = root.join("anemone.elf");
        let public_map = root.join("kernel.map");
        fs::write(&final_elf, b"new ELF").unwrap();
        fs::write(&public_elf, b"old ELF").unwrap();
        fs::write(&public_map, b"old map").unwrap();

        assert!(
            publish_verified(&final_elf, &missing_final_map, &public_elf, &public_map).is_err()
        );
        assert_eq!(fs::read(&public_elf).unwrap(), b"old ELF");
        assert_eq!(fs::read(&public_map).unwrap(), b"old map");
        fs::remove_dir_all(root).unwrap();
    }
}
