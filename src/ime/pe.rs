//! Bounded, offline PE inspection; never executes an imported DLL.
use crate::engine::Result;

pub(super) const EXPORTS: [&str; 9] = [
    "WatchCatImeStatus",
    "WatchCatImeState",
    "WatchCatImeFlags",
    "WatchCatImeDisable",
    "WatchCatImePointers",
    "WatchCatImeAbi",
    "WatchCatImeWindow",
    "WatchCatImeWindowError",
    "WatchCatImeCounters",
];
pub(super) fn verify(b: &[u8]) -> Result<()> {
    let word = |at: usize| -> Result<u16> {
        Ok(u16::from_le_bytes(
            b.get(at..at + 2)
                .ok_or("IME PE word range")?
                .try_into()
                .unwrap(),
        ))
    };
    let dword = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(
            b.get(at..at + 4)
                .ok_or("IME PE dword range")?
                .try_into()
                .unwrap(),
        ) as usize)
    };
    let header = dword(0x3c)?;
    if b.get(..2) != Some(b"MZ")
        || b.get(header..header + 4) != Some(b"PE\0\0")
        || word(header + 4)? != 0x8664
        || word(header + 22)? & 0x2000 == 0
        || word(header + 24)? != 0x20b
    {
        return Err("IME payload is not an x64 DLL".into());
    }
    let section_count = word(header + 6)? as usize;
    let optional_size = word(header + 20)? as usize;
    if section_count == 0 || section_count > 96 || optional_size < 120 {
        return Err("IME PE header bounds".into());
    }
    let sections = header + 24 + optional_size;
    let raw = |rva: usize, len: usize| -> Result<usize> {
        for i in 0..section_count {
            let section = sections + i * 40;
            let va = dword(section + 12)?;
            let size = dword(section + 16)?;
            if let Some(delta) = rva.checked_sub(va)
                && delta.checked_add(len).is_some_and(|end| end <= size)
            {
                let at = dword(section + 20)?
                    .checked_add(delta)
                    .ok_or("IME PE offset overflow")?;
                b.get(at..at.checked_add(len).ok_or("IME PE length overflow")?)
                    .ok_or("IME PE section range")?;
                return Ok(at);
            }
        }
        Err("IME export is not file backed".into())
    };
    let export_rva = dword(header + 24 + 112)?;
    let export_size = dword(header + 24 + 116)?;
    let dir = raw(export_rva, 40)?;
    let function_count = dword(dir + 20)?;
    let count = dword(dir + 24)?;
    if count == 0 || count > 8192 || function_count == 0 || function_count > 8192 {
        return Err("IME export count bounds".into());
    }
    let names = dword(dir + 32)?;
    let ordinals = dword(dir + 36)?;
    let functions = dword(dir + 28)?;
    let mut found = [false; 9];
    for i in 0..count {
        let name_rva = dword(raw(names + i * 4, 4)?)?;
        let name_at = raw(name_rva, 1)?;
        let length = b[name_at..]
            .iter()
            .take(512)
            .position(|v| *v == 0)
            .ok_or("IME export name bound")?;
        let name = std::str::from_utf8(&b[name_at..name_at + length]).map_err(|e| e.to_string())?;
        if let Some(index) = EXPORTS.iter().position(|candidate| *candidate == name) {
            let ordinal = word(raw(ordinals + i * 2, 2)?)? as usize;
            if ordinal >= function_count || found[index] {
                return Err("IME duplicate/invalid export".into());
            }
            let rva = dword(raw(functions + ordinal * 4, 4)?)?;
            if rva == 0 || rva >= export_rva && rva - export_rva < export_size {
                return Err("IME forwarded export refused".into());
            }
            let mut mapped = false;
            for section in (0..section_count).map(|i| sections + i * 40) {
                let va = dword(section + 12)?;
                let size = dword(section + 8)?.max(dword(section + 16)?);
                if rva.checked_sub(va).is_some_and(|offset| offset < size) {
                    if matches!(index, 0 | 2 | 3) && dword(section + 36)? & 0x20000000 == 0 {
                        return Err("IME function export is not executable".into());
                    }
                    mapped = true;
                    break;
                }
            }
            if !mapped {
                return Err("IME export is not mapped by the DLL".into());
            }
            if index == 5 {
                let at = raw(rva, 32)?;
                let abi = b[at..at + 32]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|v| u64::from_le_bytes(*v))
                    .collect::<Vec<_>>();
                if abi != [4, 9, 7, 1] {
                    return Err("IME DLL ABI differs from supported native profile".into());
                }
            }
            found[index] = true;
        }
    }
    if found.iter().all(|v| *v) {
        Ok(())
    } else {
        Err("IME DLL is missing required compatibility exports".into())
    }
}
