// TODO: provide some early scanning APIs that can be used before the entire
// device tree is parsed, since some critical information (e.g. memory map) is
// needed during the early boot stage, and we don't want to wait until the
// entire device tree is parsed to get that information.

use core::{alloc::Layout, ffi::CStr, ptr::NonNull};

use intrusive_collections::{LinkedList, UnsafeRef};

use crate::{
    Be32, FdtHeader, align_up,
    unflattened::{AllNodesAdapter, DeviceNode, DeviceTree, DeviceTreeHandle, Property},
};

#[derive(Debug)]
pub struct FdtParser {
    fdt_ptr: *const u32,
    cursor: *const u8,
    /// Track how many bytes is needed to allocate for the parsed structure.
    accumulated_bytes: usize,
    /// Whoa! Since we can't do dynamic allocation during parsing, we'll use a
    /// homebrewed stack to track the parent-child relationship. This field
    /// tracks how deep the stack can grow, so that we can calculate how many
    /// bytes we need to allocate for it.
    max_depth: usize,
}

#[derive(Debug, Clone, Copy)]
enum TraverseEvent<'a> {
    BeginNode { name: &'a CStr },
    EndNode,
    Prop { name: &'a CStr, value: &'a [u8] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum FdtToken {
    BeginNode = 0x1,
    EndNode = 0x2,
    Prop = 0x3,
    Nop = 0x4,
    End = 0x9,
}

#[derive(Debug)]
#[repr(C)]
struct PropHeader {
    len: Be32,
    nameoff: Be32,
}

impl TryFrom<Be32> for FdtToken {
    type Error = ();

    fn try_from(value: Be32) -> Result<Self, Self::Error> {
        match value.to_host() {
            0x1 => Ok(FdtToken::BeginNode),
            0x2 => Ok(FdtToken::EndNode),
            0x3 => Ok(FdtToken::Prop),
            0x4 => Ok(FdtToken::Nop),
            0x9 => Ok(FdtToken::End),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
struct ParsedProp<'a> {
    name: &'a CStr,
    value: &'a [u8],
}

impl FdtParser {
    pub const FDT_MAGIC: u32 = 0xd00dfeed;
    pub const FDT_VERSION: u32 = 17;
    pub const LAST_COMPATIBLE_VERSION: u32 = 16;

    /// Create a new FDT parser from a pointer to the FDT blob in memory.
    ///
    /// **This method only checks the magic number and version. Does not
    /// validate the entire structure.**
    ///
    /// # Safety
    ///
    /// The caller must ensure that `fdt_ptr` points to a valid FDT blob in
    /// memory, and that it remains valid for the lifetime of the `FdtParser`
    /// instance.
    ///
    /// Once the `FdtParser` is created, we'll safely assume the FDT blob is
    /// valid, and any violation of the FDT spec will be treated as an internal
    /// error, causing an immediate panic. This is because we don't want to
    /// introduce extra complexity to handle malformed FDT blobs, and in
    /// practice, the FDT blobs provided by the bootloader should always be
    /// well-formed. If not, the kernel actually has no chance to recover
    /// anyway.
    pub unsafe fn new(fdt_ptr: *const u32) -> Self {
        // Spec requires the FDT blob to be aligned to 8 bytes.
        if !(fdt_ptr as usize).is_multiple_of(8) {
            panic!(
                "FDT blob must be 8-byte aligned, but got {:#x}",
                fdt_ptr as usize
            );
        }

        unsafe {
            let hdr = fdt_ptr.cast::<FdtHeader>().as_ref().unwrap();
            if hdr.magic.to_host() != Self::FDT_MAGIC {
                panic!("Invalid FDT magic number");
            }
            if hdr.version.to_host() != Self::FDT_VERSION {
                panic!("Invalid FDT version");
            }
        }
        Self {
            fdt_ptr,
            cursor: fdt_ptr.cast(),
            accumulated_bytes: 0,
            max_depth: 0,
        }
    }

    /// Parse the FDT blob and construct the unflattened device tree in memory.
    ///
    /// The provided allocator will be called once with the total number of
    /// bytes needed to store the unflattened device tree, and it should return
    /// a pointer to a memory region of at least that size.
    ///
    /// The returned pointer will be used to store the unflattened device tree,
    /// and the caller is responsible for ensuring that this memory region
    /// remains valid for the lifetime of the returned `DeviceTree`.
    pub fn parse<A>(mut self, allocator: A) -> DeviceTreeHandle
    where
        A: FnOnce(Layout) -> Option<NonNull<[u8]>>,
    {
        self.calculate_arena_bytes();

        let arena = allocator(
            Layout::from_size_align(self.accumulated_bytes, align_of::<DeviceTree>()).unwrap(),
        )
        .expect("Out of memory");

        let device_tree = DeviceTree {
            header: *self.header(),
            root: NonNull::dangling(),
            all_nodes: LinkedList::new(AllNodesAdapter::new()),
        };
        let mut cur_offset = size_of::<DeviceTree>();

        cur_offset = align_up(cur_offset, align_of::<Option<NonNull<DeviceNode>>>());
        let depth_stack_ptr = unsafe {
            arena
                .as_ptr()
                .cast::<u8>()
                .add(cur_offset)
                .cast::<Option<NonNull<DeviceNode>>>()
        };
        let depth_stack_len = self.max_depth + 1;
        unsafe {
            for i in 0..depth_stack_len {
                depth_stack_ptr.add(i).write(None);
            }
        }
        cur_offset += size_of::<Option<NonNull<DeviceNode>>>() * depth_stack_len;

        unsafe {
            arena.as_ptr().cast::<DeviceTree>().write(device_tree);
            let device_tree = arena.as_ptr().cast::<DeviceTree>().as_mut().unwrap();
            self.traverse_nodes(|depth, event| match event {
                TraverseEvent::BeginNode { name } => {
                    cur_offset = align_up(cur_offset, align_of::<DeviceNode>());
                    let node_ptr = arena
                        .as_ptr()
                        .cast::<u8>()
                        .wrapping_add(cur_offset)
                        .cast::<DeviceNode>();
                    let name_ptr = node_ptr.add(1).cast::<u8>();
                    let name_bytes = name.to_bytes_with_nul();
                    debug_assert!(
                        cur_offset + size_of::<DeviceNode>() + name_bytes.len() <= arena.len()
                    );
                    node_ptr.write(DeviceNode::DANGLING);
                    name_ptr.copy_from_nonoverlapping(name.as_ptr().cast(), name_bytes.len());

                    let node = node_ptr.as_mut().unwrap();
                    node.name = NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                        name_ptr,
                        name_bytes.len(),
                    ));

                    let node_nonnull = NonNull::new_unchecked(node_ptr);
                    if depth == 0 {
                        device_tree.root = node_nonnull;
                    } else {
                        let parent = depth_stack_ptr
                            .add(depth - 1)
                            .read()
                            .expect("parent must exist for non-root node");
                        node.parent = Some(parent);
                        parent
                            .as_ptr()
                            .as_mut()
                            .unwrap()
                            .children
                            .push_back(UnsafeRef::from_raw(node_ptr));
                    }

                    depth_stack_ptr.add(depth).write(Some(node_nonnull));

                    device_tree
                        .all_nodes
                        .push_back(UnsafeRef::from_raw(node_ptr));

                    cur_offset += size_of::<DeviceNode>() + name_bytes.len();
                },
                TraverseEvent::EndNode => {},
                TraverseEvent::Prop { name, value } => {
                    cur_offset = align_up(cur_offset, align_of::<Property>());
                    let prop_ptr = arena
                        .as_ptr()
                        .cast::<u8>()
                        .wrapping_add(cur_offset)
                        .cast::<Property>();
                    let name_ptr = prop_ptr.add(1).cast::<u8>();
                    let name_bytes = name.to_bytes_with_nul();
                    let value_ptr = name_ptr.add(name_bytes.len());

                    debug_assert!(
                        cur_offset + size_of::<Property>() + name_bytes.len() + value.len()
                            <= arena.len()
                    );

                    prop_ptr.write(Property::DANGLING);
                    name_ptr.copy_from_nonoverlapping(name.as_ptr().cast(), name_bytes.len());
                    value_ptr.copy_from_nonoverlapping(value.as_ptr(), value.len());

                    let prop = prop_ptr.as_mut().unwrap();
                    prop.name = NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                        name_ptr,
                        name_bytes.len(),
                    ));
                    prop.value = NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(
                        value_ptr,
                        value.len(),
                    ));

                    let owner = depth_stack_ptr
                        .add(depth)
                        .read()
                        .expect("property must belong to a valid node");
                    owner
                        .as_ptr()
                        .as_mut()
                        .unwrap()
                        .properties
                        .push_back(UnsafeRef::from_raw(prop_ptr));

                    cur_offset += size_of::<Property>() + name_bytes.len() + value.len();
                },
            });
        }
        assert!(
            cur_offset == self.accumulated_bytes,
            "Internal error: calculated accumulated bytes {:#x} does not match actual used bytes {:#x}",
            self.accumulated_bytes,
            cur_offset
        );

        DeviceTreeHandle { arena }
    }

    fn traverse_nodes<V>(&mut self, mut visitor: V)
    where
        V: FnMut(usize, TraverseEvent<'_>),
    {
        self.cursor = self.struct_block_base();

        // depth of root node is 0, and we haven't entered the root node yet, so we
        // start with -1
        let mut depth: isize = -1;
        loop {
            self.advance();

            let token = self.eat_token();
            match token {
                FdtToken::Nop => {},
                FdtToken::BeginNode => {
                    depth += 1;
                    let name = self.eat_cstr();
                    visitor(depth as usize, TraverseEvent::BeginNode { name });
                },
                FdtToken::EndNode => {
                    visitor(depth as usize, TraverseEvent::EndNode);
                    depth -= 1;
                },
                FdtToken::Prop => {
                    let prop = self.eat_prop();
                    visitor(
                        depth as usize,
                        TraverseEvent::Prop {
                            name: prop.name,
                            value: prop.value,
                        },
                    );
                },
                FdtToken::End => {
                    debug_assert!(depth == -1, "Unexpected End token at depth {depth}");
                    break;
                },
            }
        }
    }

    /// The arena layout is as almost the same as the FDT structure block,
    /// except that we wire those nodes and properties together with linked
    /// lists, and we abandon the string block.
    fn calculate_arena_bytes(&mut self) {
        let mut max_depth = 0usize;
        self.traverse_nodes(|depth, _| {
            if depth > max_depth {
                max_depth = depth;
            }
        });
        self.max_depth = max_depth;

        let mut total = size_of::<DeviceTree>();

        total = align_up(total, align_of::<Option<NonNull<DeviceNode>>>());
        total += (self.max_depth + 1) * size_of::<Option<NonNull<DeviceNode>>>();

        self.traverse_nodes(|_, event| {
            let mut push = |size: usize, align: usize| {
                total = align_up(total, align);
                total += size;
            };

            match event {
                TraverseEvent::BeginNode { name } => {
                    push(size_of::<DeviceNode>(), align_of::<DeviceNode>());
                    push(name.to_bytes_with_nul().len(), 1);
                },
                TraverseEvent::EndNode => {},
                TraverseEvent::Prop { name, value } => {
                    push(size_of::<Property>(), align_of::<Property>());
                    push(name.to_bytes_with_nul().len(), 1);
                    push(value.len(), 1);
                },
            }
        });

        self.accumulated_bytes = total;
    }
}

impl FdtParser {
    fn struct_block_base(&self) -> *const u8 {
        self.fdt_ptr
            .cast::<u8>()
            .wrapping_add(self.header().off_dt_struct.to_host() as usize)
    }

    fn eat_token(&mut self) -> FdtToken {
        let token = unsafe { self.cursor.cast::<Be32>().read() };
        self.cursor = unsafe { self.cursor.add(4) };
        FdtToken::try_from(token).expect("Invalid FDT token")
    }

    fn eat_prop(&mut self) -> ParsedProp<'_> {
        let prop = unsafe { self.cursor.cast::<PropHeader>().as_ref().unwrap() };
        self.cursor = self.cursor.wrapping_add(size_of::<PropHeader>());

        let len = prop.len.to_host() as usize;
        let nameoff = prop.nameoff.to_host() as usize;

        let value = unsafe { core::slice::from_raw_parts(self.cursor, len) };
        self.cursor = self.cursor.wrapping_add(len);

        let name = unsafe { self.get_string(nameoff) };
        ParsedProp { name, value }
    }

    fn eat_cstr(&mut self) -> &CStr {
        let start = self.cursor;
        let cstr = unsafe { CStr::from_ptr(start.cast()) };
        let len = cstr.to_bytes_with_nul().len();
        self.cursor = self.cursor.wrapping_add(len);
        cstr
    }

    fn cursor_align_up_word(&mut self) {
        let addr = self.cursor as usize;
        let aligned_addr = (addr + 3) & !0x3;
        self.cursor = self.cursor.with_addr(aligned_addr)
    }

    /// Skip padding bytes and NOP tokens until the next meaningful token is
    /// found.
    ///
    /// This method is UB when the cursor is not within the bounds of structure
    /// block.
    fn advance(&mut self) {
        #[cfg(debug_assertions)]
        {
            let base = self.fdt_ptr as usize;
            let sstruct = base + self.header().off_dt_struct.to_host() as usize;
            let estruct = sstruct + self.header().size_dt_struct.to_host() as usize;
            let cursor_addr = self.cursor as usize;
            assert!(
                cursor_addr >= sstruct && cursor_addr < estruct,
                "Cursor out of bounds: {:#x} not in [{:#x}, {:#x})",
                cursor_addr,
                sstruct,
                estruct
            );
        }
        // align up to 4 bytes

        self.cursor_align_up_word();
        loop {
            let token = unsafe { *self.cursor.cast::<Be32>() };
            match FdtToken::try_from(token) {
                Ok(FdtToken::Nop) => {
                    self.cursor = unsafe { self.cursor.add(4) };
                },
                Err(()) => panic!("Invalid FDT token"),
                _ => break,
            }
        }
    }

    fn header(&self) -> &FdtHeader {
        unsafe { self.fdt_ptr.cast::<FdtHeader>().as_ref().unwrap() }
    }

    /// # Safety
    ///
    /// `nameoff` must be retrieved from the valid FDT structure.
    unsafe fn get_string(&self, nameoff: usize) -> &CStr {
        let offset = self.header().off_dt_strings.to_host() as usize + nameoff;
        unsafe { CStr::from_ptr(self.fdt_ptr.cast::<u8>().add(offset).cast()) }
    }
}
