pub mod asid;
pub mod crmd;
pub mod csr;
pub mod dmw;
pub mod pwc;
pub mod exception;

/// Use this macro to generate compact bitfield accessors for a 64-bit value
/// stored in `self.0`.
///
/// Variants:
/// - **`bitflags`**: Create typed getter and setter using a bitflags-like type.
///   
///     Invoke as 
///     ```
///     impl_bits64!(bitflags, <basetype>, <name>, <BitflagsType>, <start>, <end>) 
///     ```
///     
///     Provide `<basetype>` for casting (for example `u8`),
///         and supply a bitflags type that implements `bits() -> uN` and
///         `from_bits_retain(uN)`.
///
/// - **`value`**: Create typed getter and setter for a wrapper/value type that
///   converts to/from a raw integer.
///
///     Invoke as
///     ```
///     impl_bits64!(value, <basetype>, <name>, <ValueType>, <start>, <end>)
///     ```
///
///     The `<ValueType>` must provide `from_value_or_default(uN) -> Self` and
///         `value(&self) -> uN` (where `uN` matches `<basetype>`).
///
/// - **`number`**: Create numeric accessors.
///
///     Invoke as
///     ```
///     impl_bits64!(number, <name>, <Type>, <start>, <end>)
///     ```
///
///     Which creates `read_<name>()` and
///         `set_<name>` that operate on `<Type>`.
///
/// - **`bool`**: Create boolean accessors for a single bit.
///
///     Invoke as
///     ```
///     impl_bits64!(bool, <name>, <bit>)
///     ```
///
///     Which creates `read_<name>() -> bool` and `set_<name>(bool)`.
#[macro_export]
macro_rules! impl_bits64 {
    (bitflags, $basetype: ident, $name: ident, $type: ident, $st: expr, $ed: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u64 = (0xFFFF_FFFF_FFFF_FFFF << $st) & (0xFFFF_FFFF_FFFF_FFFF >> (64 - $ed));
            const [<SHIFT_ $name:upper>]: u64 = $st;
            pub const fn $name(&self)->$type{
                $type::from_bits_retain(((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $basetype)
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value.bits() as u64) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
    (value, $basetype: ident, $name: ident, $type: ident, $st: expr, $ed: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u64 = (0xFFFF_FFFF_FFFF_FFFF << $st) & (0xFFFF_FFFF_FFFF_FFFF >> (64 - $ed));
            const [<SHIFT_ $name:upper>]: u64 = $st;
            pub const fn $name(&self)->$type{
                $type::from_value_or_default(((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $basetype)
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value.value() as u64) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
    (number, $name: ident, $type: ident, $st: expr, $ed: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u64 = (0xFFFF_FFFF_FFFF_FFFF << $st) & (0xFFFF_FFFF_FFFF_FFFF >> (64 - $ed));
            const [<SHIFT_ $name:upper>]: u64 = $st;
            pub const fn $name(&self)->$type{
                ((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $type
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value as u64) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
    (bool, $name: ident, $st: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u64 = (0xFFFF_FFFF_FFFF_FFFF << $st) & (0xFFFF_FFFF_FFFF_FFFF >> (64 - ($st + 1)));
            const [<SHIFT_ $name:upper>]: u64 = $st;
            pub const fn $name(&self)->bool{
                ((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) == 1
            }
            pub const fn [<set_ $name>](&mut self, value: bool){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((if value { 1 } else { 0 }) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
}

/// Use this macro to generate compact bitfield accessors for a 32-bit value
/// stored in `self.0`.
///
/// Variants:
/// - **`bitflags`**: Create typed getter and setter using a bitflags-like type.
///
///     Invoke as
///     ```
///     impl_bits32!(bitflags, <basetype>, <name>, <BitflagsType>, <start>, <end>)
///     ```
///
///     Provide `<basetype>` for casting (for example `u8`),
///         and supply a bitflags type that implements `bits() -> uN` and
///         `from_bits_retain(uN)`.
///
/// - **`value`**: Create typed getter and setter for a wrapper/value type that
///   converts to/from a raw integer.
///
///     Invoke as
///     ```
///     impl_bits32!(value, <basetype>, <name>, <ValueType>, <start>, <end>)
///     ```
///
///     The `<ValueType>` must provide `from_value_or_default(uN) -> Self` and
///         `value(&self) -> uN` (where `uN` matches `<basetype>`).
///
/// - **`number`**: Create numeric accessors.
///
///     Invoke as
///     ```
///     impl_bits32!(number, <name>, <Type>, <start>, <end>)
///     ```
///
///     Which creates `read_<name>()` and
///         `set_<name>` that operate on `<Type>`.
///
/// - **`bool`**: Create boolean accessors for a single bit.
///
///     Invoke as
///     ```
///     impl_bits32!(bool, <name>, <bit>)
///     ```
///
///     Which creates `read_<name>() -> bool` and `set_<name>(bool)`.
#[macro_export]
macro_rules! impl_bits32 {
    (bitflags, $basetype: ident, $name: ident, $type: ident, $st: expr, $ed: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u32 = (0xFFFF_FFFF << $st) & (0xFFFF_FFFF >> (32 - $ed));
            const [<SHIFT_ $name:upper>]: u32 = $st;
            pub const fn $name(&self)->$type{
                $type::from_bits_retain(((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $basetype)
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value.bits() as u32) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
    (value, $basetype: ident, $name: ident, $type: ident, $st: expr, $ed: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u32 = (0xFFFF_FFFF << $st) & (0xFFFF_FFFF >> (32 - $ed));
            const [<SHIFT_ $name:upper>]: u32 = $st;
            pub const fn $name(&self)->$type{
                $type::from_value_or_default(((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $basetype)
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value.value() as u32) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
    (number, $name: ident, $type: ident, $st: expr, $ed: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u32 = (0xFFFF_FFFF << $st) & (0xFFFF_FFFF >> (32 - $ed));
            const [<SHIFT_ $name:upper>]: u32 = $st;
            pub const fn $name(&self)->$type{
                ((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $type
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value as u32) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
    (bool, $name: ident, $st: expr) => {
        paste::paste! {
            const [<MASK_ $name:upper>]: u32 = (0xFFFF_FFFF << $st) & (0xFFFF_FFFF >> (32 - ($st + 1)));
            const [<SHIFT_ $name:upper>]: u32 = $st;
            pub const fn $name(&self)->bool{
                ((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) == 1
            }
            pub const fn [<set_ $name>](&mut self, value: bool){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((if value { 1 } else { 0 }) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
}

/// Generate register-level read/write helper functions that operate on CSR
/// wrapper types.
///
/// Invoke as
/// ```
/// impl_rw!(<csr_module>, <field_name>, <FieldType>)
/// ```.
///
/// This expands to:
/// - `set_<field_name>(value: <FieldType>)`: read CSR, update field, write back.
/// - `read_<field_name>() -> <FieldType>`: read CSR and return field value.
///
/// The referenced `csr::<csr_module>` module must provide:
/// - `unsafe fn csr_read() -> RegType`
/// - `unsafe fn csr_write(RegType)`
///
/// And `RegType` must provide `set_<field_name>(<FieldType>)` and
/// `read_<field_name>() -> <FieldType>`.
#[macro_export]
macro_rules! impl_rw {
    ($reg:ident, $name:ident, $type:ident) => {
        paste::paste! {
            pub fn [<set_ $name>](value: $type) {
                let mut crmd_csr = unsafe { $crate::reg::csr::$reg::csr_read() };
                crmd_csr.[<set_ $name>](value);
                unsafe {
                    $crate::reg::csr::$reg::csr_write(crmd_csr);
                }
            }

            pub fn [<read_ $name>]() -> $type {
                let crmd_csr = unsafe { $crate::reg::csr::$reg::csr_read() };
                crmd_csr.$name()
            }
        }

    };
}