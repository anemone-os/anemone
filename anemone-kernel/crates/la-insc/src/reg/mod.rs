pub mod csr;
pub mod dmw;

/// Use this macro to generate compact bitfield accessors for a 64-bit value
/// stored in `self.0`.
///
/// Variants:
/// - **`bitflags`**: Create typed getter and setter using a bitflags-like type.
///   Invoke as
///   ```
///     impl_bits64!(bitflags, <basetype>, <name>, <BitflagsType>, <start>, <end>)
///   ```
///   Provide `<basetype>` for casting (for example `u8`), and supply a
///   bitflags type that implements `bits() -> uN` and `from_bits_retain(uN)`.
/// 
/// - **`value`**: Create typed getter and setter for a wrapper/value type that
///   converts to/from a raw integer. Invoke as
///   ```
///     impl_bits64!(value, <basetype>, <name>, <ValueType>, <start>, <end>)
///   ```
///   The `<ValueType>` must provide `from_value_or_default(uN) -> Self` and
///   `value(&self) -> uN` (where `uN` matches `<basetype>`).
/// 
/// - **`number`**: Create numeric accessors. Invoke as
///   ```
///     impl_bits64!(number, <name>, <Type>, <start>, <end>)
///   ```
///   which creates `read_<name>()` and `set_<name>` that operate on `<Type>`.
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
            pub const fn [<read_ $name>](&self)->$type{
                ((self.0 & Self::[<MASK_ $name:upper>]) >> Self::[<SHIFT_ $name:upper>]) as $type
            }
            pub const fn [<set_ $name>](&mut self, value: $type){
                let mut val = self.0 & !Self::[<MASK_ $name:upper>];
                val |= ((value as u64) << Self::[<SHIFT_ $name:upper>]) & Self::[<MASK_ $name:upper>];
                self.0 = val;
            }
        }
    };
}
