#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::string::String;

use core::{ffi::CStr, ptr::NonNull};

use intrusive_collections::{LinkedList, LinkedListLink, UnsafeRef, intrusive_adapter};

use crate::{FdtHeader, align_up};

intrusive_adapter!(
    pub(crate) PropertyAdapter = UnsafeRef<Property>: Property { node_properties => LinkedListLink }
);

intrusive_adapter!(
    pub(crate) DeviceNodeAdapter = UnsafeRef<DeviceNode>: DeviceNode { node_children => LinkedListLink }
);

/// Represents a property of a device node.
#[derive(Debug)]
pub struct Property {
    pub(crate) name: NonNull<[u8]>,
    pub(crate) value: NonNull<[u8]>,
    pub(crate) node_properties: LinkedListLink,
}

impl Property {
    /// Get the name of this property as a string slice, without the trailing
    /// null byte.
    ///
    /// Since ASCII is a subset of UTF-8, and the spec requires that all
    /// characters in FDT be valid ASCII, this is always valid UTF-8, so we can
    /// safely convert it to [str].
    pub fn name(&self) -> &str {
        unsafe {
            let bytes = self.name.as_ref();
            let len = bytes.len();
            str::from_utf8(&bytes[..len - 1])
                .expect("Property name must be valid UTF-8 and end with a null byte")
        }
    }

    /// Get the value of this property as a byte slice. The raw representation.
    pub fn value_as_bytes(&self) -> &[u8] {
        unsafe { self.value.as_ref() }
    }

    // following methods match value interpretation rules defined in the FDT spec.

    /// Interpret the value of this property as a string slice, if it is valid
    /// UTF-8.
    ///
    /// Note that the bytes will first be interpreted as a C string to ensure
    /// there is no null bytes except the trailing one (None will be returned
    /// otherwise), which will be stripped then, and the remaining bytes
    /// will be interpreted as UTF-8 for easy use in Rust.
    ///
    /// If the remaing bytes are not valid UTF-8, return None.
    pub fn value_as_string(&self) -> Option<&str> {
        let bytes = self.value_as_bytes();
        if bytes.last() != Some(&0) {
            return None;
        }
        CStr::from_bytes_with_nul(bytes)
            .ok()
            .and_then(|cstr| cstr.to_str().ok())
    }

    /// Interpret the value of this property as an array of string slices.
    ///
    /// The iterator will skip malformed strings until next valid string is
    /// found, or the end of the value is reached. For what is considered a
    /// valid string, see [Self::value_as_string].
    pub fn value_as_stringlist(&self) -> Option<StringList<'_>> {
        let bytes = self.value_as_bytes();
        // check whether this value can be interpreted as a sequence of C strings first
        if bytes.last() != Some(&0) {
            return None;
        }
        Some(StringList { bytes, idx: 0 })
    }

    /// Interpret the value of this property as a big-endian u32, if it is
    /// exactly 4 bytes long.
    pub fn value_as_u32(&self) -> Option<u32> {
        let bytes = self.value_as_bytes();
        if bytes.len() != 4 {
            return None;
        }
        Some(u32::from_be_bytes(bytes.try_into().unwrap()))
    }

    /// Interpret the value of this property as a big-endian u64, if it is
    /// exactly 8 bytes long.
    pub fn value_as_u64(&self) -> Option<u64> {
        let bytes = self.value_as_bytes();
        if bytes.len() != 8 {
            return None;
        }
        Some(u64::from_be_bytes(bytes.try_into().unwrap()))
    }

    /// Interpret the value of this property as a phandle, which is just a
    /// big-endian u32.
    pub fn value_as_phandle(&self) -> Option<u32> {
        self.value_as_u32()
    }

    /// Interpret the value of this property as an array of big-endian u32, if
    /// its length is a multiple of 4.
    pub fn value_as_u32_array(&self) -> Option<PropEncodedArray<'_, U32ArrayEncoding>> {
        let bytes = self.value_as_bytes();
        if !bytes.len().is_multiple_of(4) {
            return None;
        }
        Some(PropEncodedArray::new(bytes, U32ArrayEncoding))
    }

    /// Interpret the value of this property as an array of big-endian u64, if
    /// its length is a multiple of 8.
    pub fn value_as_u64_array(&self) -> Option<PropEncodedArray<'_, U64ArrayEncoding>> {
        let bytes = self.value_as_bytes();
        if !bytes.len().is_multiple_of(8) {
            return None;
        }
        Some(PropEncodedArray::new(bytes, U64ArrayEncoding))
    }

    /// Interpret the value of this property as an array of items with encoding
    /// E, if its length is a multiple of one encoded item in E.
    ///
    /// Some convinient encodings, such as big-endian u32 and u64, are already
    /// provided, and users can also define their own encoding by implementing
    /// the [PropEncoding] trait.
    pub fn value_as_prop_encoded_array<E: PropEncoding>(
        &self,
        enc: E,
    ) -> Option<PropEncodedArray<'_, E>> {
        let bytes = self.value_as_bytes();
        let item_encoded_len = enc.encoded_len();
        if !bytes.len().is_multiple_of(item_encoded_len) {
            return None;
        }
        Some(PropEncodedArray::new(bytes, enc))
    }
}

/// Specifies the encoding of a prop-encoded array.
pub trait PropEncoding {
    type Item: Copy;

    /// Size in bytes of one encoded item.
    fn encoded_len(&self) -> usize;

    /// Length of `bytes` is guaranteed to be equal to [Self::encoded_len].
    fn decode(&self, bytes: &[u8]) -> Option<Self::Item>;
}

#[derive(Debug, Clone)]
pub struct PropEncodedArray<'a, E: PropEncoding> {
    bytes: &'a [u8],
    enc: E,
}

#[derive(Debug, Clone)]
pub struct PropEncodedArrayIter<'a, 'b, E: PropEncoding> {
    array: &'a PropEncodedArray<'b, E>,
    idx: usize,
}

impl<'a, E: PropEncoding> PropEncodedArray<'a, E> {
    pub fn new(bytes: &'a [u8], enc: E) -> Self {
        Self { bytes, enc }
    }

    pub fn iter(&self) -> PropEncodedArrayIter<'_, 'a, E> {
        PropEncodedArrayIter {
            array: self,
            idx: 0,
        }
    }
}

impl<'a, 'b, E: PropEncoding> Iterator for PropEncodedArrayIter<'a, 'b, E> {
    type Item = E::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.array.bytes.len() {
            return None;
        }
        let end = self.idx + self.array.enc.encoded_len();
        let chunk = self.array.bytes.get(self.idx..end)?;
        self.idx = end;
        self.array.enc.decode(chunk)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct U32ArrayEncoding;
impl PropEncoding for U32ArrayEncoding {
    type Item = u32;
    #[inline]
    fn encoded_len(&self) -> usize {
        4
    }

    #[inline]
    fn decode(&self, bytes: &[u8]) -> Option<Self::Item> {
        Some(u32::from_be_bytes(bytes.try_into().unwrap()))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct U64ArrayEncoding;
impl PropEncoding for U64ArrayEncoding {
    type Item = u64;
    #[inline]
    fn encoded_len(&self) -> usize {
        8
    }

    #[inline]
    fn decode(&self, bytes: &[u8]) -> Option<Self::Item> {
        Some(u64::from_be_bytes(bytes.try_into().unwrap()))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RangesEncoding {
    // these two fields are not implemented via const generics cz they are almost always known at
    // runtime instead of compile time.
    child_cells: Cells,
    parent_cells: Cells,
}

impl RangesEncoding {
    pub fn new(child_cells: Cells, parent_cells: Cells) -> Self {
        Self {
            child_cells,
            parent_cells,
        }
    }
}

impl PropEncoding for RangesEncoding {
    /// (child-bus-address, parent-bus-address, length)
    ///
    /// u64 is used to hold both 32-bit and 64-bit values, and the actual size
    /// of each field is determined by the `child_cells` and `parent_cells`
    /// fields in this encoding.
    type Item = (u64, u64, u64);

    #[inline]
    fn encoded_len(&self) -> usize {
        ((self.child_cells.addr_cells + self.parent_cells.addr_cells + self.child_cells.size_cells)
            * 4) as usize
    }

    fn decode(&self, bytes: &[u8]) -> Option<Self::Item> {
        fn decode_be_u64(bytes: &[u8]) -> u64 {
            debug_assert!(bytes.len() <= 8);
            bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64)
        }

        let child_addr_end = (self.child_cells.addr_cells * 4) as usize;
        let parent_addr_end =
            ((self.child_cells.addr_cells + self.parent_cells.addr_cells) * 4) as usize;
        let length_end = self.encoded_len();

        let child_addr_bytes = bytes.get(0..child_addr_end).unwrap();
        let parent_addr_bytes = bytes.get(child_addr_end..parent_addr_end).unwrap();
        let length_bytes = bytes.get(parent_addr_end..length_end).unwrap();

        let child_addr = decode_be_u64(child_addr_bytes);
        let parent_addr = decode_be_u64(parent_addr_bytes);
        let length = decode_be_u64(length_bytes);
        Some((child_addr, parent_addr, length))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RegEncoding {
    cells: Cells,
}

impl RegEncoding {
    pub fn new(cells: Cells) -> Self {
        Self { cells }
    }
}

impl PropEncoding for RegEncoding {
    /// (address, length)
    type Item = (u64, u64);

    fn encoded_len(&self) -> usize {
        ((self.cells.addr_cells + self.cells.size_cells) * 4) as usize
    }

    fn decode(&self, bytes: &[u8]) -> Option<Self::Item> {
        fn decode_be_u64(bytes: &[u8]) -> u64 {
            debug_assert!(bytes.len() <= 8);
            bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64)
        }

        let addr_end = (self.cells.addr_cells * 4) as usize;
        let length_end = self.encoded_len();

        let addr_bytes = bytes.get(0..addr_end).unwrap();
        let length_bytes = bytes.get(addr_end..length_end).unwrap();

        let addr = decode_be_u64(addr_bytes);
        let length = decode_be_u64(length_bytes);
        Some((addr, length))
    }
}

#[derive(Debug, Clone)]
pub struct StringList<'a> {
    bytes: &'a [u8],
    idx: usize,
}

impl<'a> Iterator for StringList<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        while self.idx < self.bytes.len() {
            let start = self.idx;
            while self.idx < self.bytes.len() && self.bytes[self.idx] != 0 {
                self.idx += 1;
            }
            if self.idx == self.bytes.len() {
                // no trailing null byte, this is a malformed string, skip it
                break;
            }
            // null terminator, skip
            self.idx += 1;

            let cstr_bytes = &self.bytes[start..self.idx];
            if let Ok(cstr) = CStr::from_bytes_with_nul(cstr_bytes) {
                if let Ok(str) = cstr.to_str() {
                    return Some(str);
                }
            }
            // otherwise, this is a malformed string, skip it and continue to
            // find the next one
        }
        None
    }
}

impl Property {
    pub(crate) const DANGLING: Self = Self {
        name: unsafe {
            NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                core::ptr::dangling_mut(),
                0,
            ))
        },
        value: unsafe {
            NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                core::ptr::dangling_mut(),
                0,
            ))
        },
        node_properties: LinkedListLink::new(),
    };
}

/// Represents a node in the unflattened device tree.
#[derive(Debug)]
pub struct DeviceNode {
    /// ASCII encoded name of this node, including the trailing null byte.
    pub(crate) name: NonNull<[u8]>,
    pub(crate) properties: LinkedList<PropertyAdapter>,
    /// For root node, the value is None.
    pub(crate) parent: Option<NonNull<DeviceNode>>,
    pub(crate) children: LinkedList<DeviceNodeAdapter>,
    pub(crate) node_children: LinkedListLink,
    /// Linked in the global list of all nodes, for easy traversal.
    pub(crate) node_all: LinkedListLink,
    // TODO:
    // node_sibling: LinkedListLink,
}

impl DeviceNode {
    /// Get the name of this node as a string slice, without the trailing null
    /// byte.
    ///
    /// Since ASCII is a subset of UTF-8, this is always valid UTF-8, so we can
    /// safely convert it to [str].
    pub fn full_name(&self) -> &str {
        unsafe {
            let bytes = self.name.as_ref();
            let len = bytes.len();
            str::from_utf8(&bytes[..len - 1])
                .expect("Node name must be valid UTF-8 and end with a null byte")
        }
    }

    /// Get the name of this node without the unit address part.
    ///
    /// Note: the name of the root node is empty.
    pub fn name(&self) -> &str {
        self.full_name().split('@').next().unwrap_or("")
    }

    /// Get the unit address part of this node's name, if it has one.
    pub fn unit_addr(&self) -> Option<&str> {
        let full_name = self.full_name();
        full_name.split('@').nth(1)
    }

    /// Get an iterator over the properties of this node.
    pub fn properties(&self) -> impl Iterator<Item = &Property> {
        self.properties.iter()
    }

    /// Get an iterator over the children of this node.
    ///
    /// Return None for root node.
    pub fn parent(&self) -> Option<&DeviceNode> {
        self.parent.map(|p| unsafe { p.as_ref() })
    }

    /// Get an iterator over the children of this node.
    pub fn children(&self) -> impl Iterator<Item = &DeviceNode> {
        self.children.iter()
    }

    /// Get the full path of this node, starting from the root, with each
    /// component separated by a '/'.
    ///
    /// **This method depends on the `alloc` feature being enabled.**
    #[cfg(feature = "alloc")]
    pub fn path(&self) -> String {
        let mut path = String::new();
        if let Some(parent) = self.parent() {
            path.push_str(&parent.path());
        }
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(self.full_name());
        path
    }

    // following methods are for convenience, to get commonly used properties in a
    // more structured way.

    /// Get the #size-cells and #address-cells values in this node, or the
    /// default values if they are not specified.
    pub fn cells(&self) -> Cells {
        let mut cells = Cells::default();
        for prop in self.properties() {
            match prop.name() {
                "#size-cells" => {
                    if let Some(size_cells) = prop.value_as_u32() {
                        cells.size_cells = size_cells;
                    }
                },
                "#address-cells" => {
                    if let Some(addr_cells) = prop.value_as_u32() {
                        cells.addr_cells = addr_cells;
                    }
                },
                _ => {},
            }
        }
        cells
    }

    /// Get the #size-cells and #address-cells values in parent node.
    pub fn cells_self(&self) -> Cells {
        self.parent().map(|p| p.cells()).unwrap_or_default()
    }

    /// Get the phandle value of this node, if it has a "phandle" property with
    /// a valid phandle value.
    pub fn phandle(&self) -> Option<u32> {
        for prop in self.properties() {
            if prop.name() == "phandle" {
                return prop.value_as_phandle();
            }
        }
        None
    }

    ///
    pub fn reg_raw(&self) -> Option<&[u8]> {
        for prop in self.properties() {
            if prop.name() == "reg" {
                return Some(prop.value_as_bytes());
            }
        }
        None
    }

    /// Get the "compatible" property of this node, which is a list of strings
    /// that identify the compatibility of this node, if it exists.
    pub fn compatible(&self) -> Option<StringList<'_>> {
        for prop in self.properties() {
            if prop.name() == "compatible" {
                return prop.value_as_stringlist();
            }
        }
        None
    }

    /// Get the "status" property of this node, which indicates the operational
    /// status of this node, or [DeviceStatus::Okay] if the property does not
    /// exist.
    pub fn status(&self) -> DeviceStatus {
        for prop in self.properties() {
            if prop.name() == "status" {
                if let Some(status_str) = prop.value_as_string() {
                    return match status_str {
                        "okay" => DeviceStatus::Okay,
                        "disabled" => DeviceStatus::Disabled,
                        "reserved" => DeviceStatus::Reserved,
                        s if s.starts_with("fail-") => DeviceStatus::FailSSS,
                        "fail" => DeviceStatus::Fail,
                        _ => DeviceStatus::Fail, // treat unrecognized status as fail
                    };
                }
            }
        }
        DeviceStatus::Okay
    }

    /// Get the "ranges" property of this node (often a bus node), which
    /// describes the address mapping between this node and its parent, if
    /// it exists.
    ///
    /// If [None] is returned, it means there is no mapping between this node
    /// and its parent, and the address space of this node is independent of its
    /// parent (e.g. i2c, spi bus).
    ///
    /// If length of the returned array is 0, it means the "ranges" property has
    /// an <empty> value, which indicates that the address space of this node is
    /// exactly the same as its parent.
    pub fn ranges(&self) -> Option<PropEncodedArray<'_, RangesEncoding>> {
        let child_cells = self.cells();
        let parent_cells = self.cells_self();
        let enc = RangesEncoding::new(child_cells, parent_cells);
        for prop in self.properties() {
            if prop.name() == "ranges" {
                return prop.value_as_prop_encoded_array(enc);
            }
        }
        None
    }

    /// Get the "reg" property of this node, which describes the address and
    /// size of the resources of this node, if it exists.
    pub fn reg(&self) -> Option<PropEncodedArray<'_, RegEncoding>> {
        let cells = self.cells_self();
        let enc = RegEncoding::new(cells);
        for prop in self.properties() {
            if prop.name() == "reg" {
                return prop.value_as_prop_encoded_array(enc);
            }
        }
        None
    }
}

/// The status property indicates the operational status of a device. The lack
/// of a status property should be treated as if the property existed with the
/// value of "okay".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Indicates the device is operational.
    Okay,
    ///Indicates that the device is not presently operational, but it might
    /// become operational in the future (for example, something is not plugged
    /// in, or switched off).
    ///
    /// Refer to the device binding for details on what disabled means for a
    /// given device.
    Disabled,
    /// Indicates that the device is operational, but should not be used.
    /// Typically this is used for devices that are controlled by another
    /// software component, such as platform firmware.
    Reserved,
    /// Indicates that the device is not operational. A serious error was
    /// detected in the device, and it is unlikely to become operational without
    /// repair.
    Fail,
    /// Indicates that the device is not operational. A serious error was
    /// detected in the device and it is unlikely to becomeoperational without
    /// repair. The sss portion of the value is specific to the device and
    /// indicates the error condition detected.
    FailSSS,
}

#[derive(Debug, Clone, Copy)]
pub struct Cells {
    pub size_cells: u32,
    pub addr_cells: u32,
}

impl Default for Cells {
    fn default() -> Self {
        // As spec defines, "ADTSpec-compliantbootprogramshall supply
        // #address-cells and #size-cells on all nodes that have children.
        // If missing, a client program should assume a default value of 2 for
        // #address-cells, and a value of 1 for #size cells."
        Cells {
            size_cells: 1,
            addr_cells: 2,
        }
    }
}

impl DeviceNode {
    pub(crate) const DANGLING: Self = Self {
        name: unsafe {
            NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                core::ptr::dangling_mut(),
                0,
            ))
        },
        properties: LinkedList::new(PropertyAdapter::NEW),
        parent: None,
        children: LinkedList::new(DeviceNodeAdapter::NEW),
        node_children: LinkedListLink::new(),
        node_all: LinkedListLink::new(),
    };
}

intrusive_adapter!(
    pub(crate) AllNodesAdapter = UnsafeRef<DeviceNode>: DeviceNode { node_all => LinkedListLink }
);

/// Handle to the unflattened device tree. The actual data is stored in the
/// allocated arena.
#[derive(Debug)]
pub struct DeviceTreeHandle {
    pub(crate) arena: NonNull<[u8]>,
}

// SAFETY: `DeviceTreeHandle` is an owning handle to an arena allocated during
// parse, and the public API only provides shared references to immutable tree
// data. Moving the handle across threads transfers ownership of that arena
// pointer and does not permit concurrent mutable access through this type.
unsafe impl Send for DeviceTreeHandle {}

impl DeviceTreeHandle {
    fn device_tree(&self) -> &DeviceTree {
        let start = self.arena.as_ptr().cast::<u8>() as usize;
        let hdr_addr = align_up(start, align_of::<DeviceTree>());
        let ptr = self.arena.as_ptr().with_addr(hdr_addr).cast::<DeviceTree>();
        unsafe { ptr.as_ref().unwrap() }
    }

    /// Get the FDT header of this device tree.
    pub fn fdt_header(&self) -> &FdtHeader {
        &self.device_tree().header
    }

    /// Get the root node of this device tree.
    pub fn root(&self) -> &DeviceNode {
        unsafe { self.device_tree().root.as_ref() }
    }

    /// Get an iterator over all nodes in this device tree.
    pub fn all_nodes(&self) -> impl Iterator<Item = &DeviceNode> {
        self.device_tree().all_nodes.iter()
    }

    // following methods are for convenience, to get commonly used information in a
    // more structured way.

    /// Get the "model" property of the root node. For a valid device tree, this
    /// should always be present.
    ///
    /// Quoted from spec:
    ///
    /// Specifies a string that uniquely identifies the model of the system
    /// board.
    ///
    /// The recommended format is "manufacturer,model-number".
    pub fn model(&self) -> Option<&str> {
        for prop in self.root().properties() {
            if prop.name() == "model" {
                return prop.value_as_string();
            }
        }
        None
    }

    /// Get the "compatible" property of the root node, which is a list of
    /// strings that identify the compatibility of this device tree. For a
    /// valid device tree, this should always be present.
    ///
    /// Quoted from spec:
    ///
    /// Specifies a list of platform architectures with which this platform is
    /// compatible. This property can be used by operating systems in selecting
    /// platform-specific code.
    ///
    /// The recommended form of the property value is: "manufacturer, model"
    ///
    /// For example: compatible = "fsl, mpc8572ds"
    pub fn compatible(&self) -> Option<StringList<'_>> {
        for prop in self.root().properties() {
            if prop.name() == "compatible" {
                return prop.value_as_stringlist();
            }
        }
        None
    }
}

/// Header located at the beginning of the allocated arena.
#[derive(Debug)]
pub(crate) struct DeviceTree {
    pub(crate) header: FdtHeader,
    pub(crate) root: NonNull<DeviceNode>,
    pub(crate) all_nodes: LinkedList<AllNodesAdapter>,
}
