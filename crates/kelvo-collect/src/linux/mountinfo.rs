//! `/proc/self/mountinfo` parsing, shared by the filesystem collector and the disk
//! inventory (which disk backs `/`).

/// One mountinfo line, with the fields the collectors use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Mount {
    /// `st_dev` of the filesystem. btrfs (and other multi-device filesystems) report an
    /// anonymous major 0 shared by every subvolume mount.
    pub(super) dev: (u32, u32),
    /// Where it is mounted, with the kernel's octal escapes (`\040`) undone.
    pub(super) point: String,
    pub(super) fstype: String,
    /// The mount source (`/dev/nvme0n1p6`, `tmpfs`, `pool/data`).
    pub(super) source: String,
}

/// Every parsable line of a mountinfo file, in file order. Malformed lines are skipped.
pub(super) fn parse(text: &str) -> Vec<Mount> {
    text.lines().filter_map(parse_line).collect()
}

/// `36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue`:
/// id, parent, major:minor, root, mount point, options, optional fields up to `-`, then
/// fstype, source, super options (proc(5)).
fn parse_line(line: &str) -> Option<Mount> {
    let mut it = line.split_ascii_whitespace();
    let _id = it.next()?;
    let _parent = it.next()?;
    let (major, minor) = it.next()?.split_once(':')?;
    let dev = (major.parse().ok()?, minor.parse().ok()?);
    let _root = it.next()?;
    let point = unescape(it.next()?);
    it.find(|f| *f == "-")?;
    let fstype = it.next()?.to_owned();
    let source = unescape(it.next()?);
    Some(Mount {
        dev,
        point,
        fstype,
        source,
    })
}

/// Undoes the kernel's `\ooo` escapes for space, tab, newline and backslash.
fn unescape(field: &str) -> String {
    if !field.contains('\\') {
        return field.to_owned();
    }
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let octal = bytes
            .get(i + 1..i + 4)
            .filter(|d| b == b'\\' && d.iter().all(|c| (b'0'..=b'7').contains(c)))
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        if let Some(c) = octal {
            out.push(c);
            i += 4;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_fields_and_escapes() {
        let m = parse(
            "54 2 0:41 /root / rw,relatime shared:1 - btrfs /dev/nvme0n1p6 rw,subvol=/root\n\
             77 54 8:1 / /media/My\\040Drive rw master:3 shared:110 - exfat /dev/sda1 rw\n\
             garbage line\n",
        );
        assert_eq!(m.len(), 2);
        assert_eq!(
            m[0],
            Mount {
                dev: (0, 41),
                point: "/".into(),
                fstype: "btrfs".into(),
                source: "/dev/nvme0n1p6".into(),
            }
        );
        assert_eq!(m[1].point, "/media/My Drive");
        assert_eq!(m[1].dev, (8, 1));
    }

    #[test]
    fn unescape_leaves_lone_backslashes() {
        assert_eq!(unescape(r"a\134b"), r"a\b");
        assert_eq!(unescape(r"a\9b"), r"a\9b");
        assert_eq!(unescape(r"end\04"), r"end\04");
    }
}
