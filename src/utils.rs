
#[macro_export]
macro_rules! pack_bilge {
    ($t:ty) => {
    
        impl packbytes::ToBytes for $t {
            type Bytes = [u8; core::mem::size_of::<$t>()];
            
            fn to_le_bytes(self) -> Self::Bytes {
                self.value.value().to_le_bytes()
            }
            fn to_be_bytes(self) -> Self::Bytes {
                self.value.value().to_be_bytes()
            }
        }
        impl packbytes::FromBytes for $t {
            type Bytes = [u8; core::mem::size_of::<$t>()];
            
            fn from_le_bytes(bytes: Self::Bytes) -> Self {
                <$t>::from(<$t as bilge::Bitsized>::ArbitraryInt::from_be_bytes(bytes))
            }
            fn from_be_bytes(bytes: Self::Bytes) -> Self {
                <$t>::from(<$t as bilge::Bitsized>::ArbitraryInt::from_be_bytes(bytes))
            }
        }
    };
}

#[macro_export]
macro_rules! pack_enum {
    ($t:ty) => {
    
        impl packbytes::ToBytes for $t {
            type Bytes = [u8; core::mem::size_of::<$t>()];
            
            fn to_le_bytes(self) -> Self::Bytes {
                <$t as bilge::Bitsized>::ArbitraryInt::from(self).to_le_bytes()
            }
            fn to_be_bytes(self) -> Self::Bytes {
                <$t as bilge::Bitsized>::ArbitraryInt::from(self).to_be_bytes()
            }
        }
        impl packbytes::FromBytes for $t {
            type Bytes = [u8; core::mem::size_of::<$t>()];
            
            fn from_le_bytes(bytes: Self::Bytes) -> Self {
                <$t>::from(<$t as bilge::Bitsized>::ArbitraryInt::from_be_bytes(bytes))
            }
            fn from_be_bytes(bytes: Self::Bytes) -> Self {
                <$t>::from(<$t as bilge::Bitsized>::ArbitraryInt::from_be_bytes(bytes))
            }
        }
    };
}
