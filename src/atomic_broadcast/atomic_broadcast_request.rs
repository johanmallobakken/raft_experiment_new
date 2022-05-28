use protobuf::Message as Message_imported_for_functions;
use protobuf::ProtobufEnum as ProtobufEnum_imported_for_functions;
use protobuf::descriptor::file_descriptor_proto;

#[derive(PartialEq,Clone,Default)]
pub struct AtomicBroadcastRequest {
    // message fields
    pub algorithm: ::std::string::String,
    pub number_of_nodes: u64,
    pub number_of_proposals: u64,
    pub concurrent_proposals: u64,
    pub reconfiguration: ::std::string::String,
    pub reconfig_policy: ::std::string::String,
    // special fields
    pub unknown_fields: ::protobuf::UnknownFields,
    pub cached_size: ::protobuf::CachedSize,
}

impl<'a> ::std::default::Default for &'a AtomicBroadcastRequest {
    fn default() -> &'a AtomicBroadcastRequest {
        <AtomicBroadcastRequest as ::protobuf::Message>::default_instance()
    }
}

impl AtomicBroadcastRequest {
    pub fn new() -> AtomicBroadcastRequest {
        ::std::default::Default::default()
    }

    // string algorithm = 1;


    pub fn get_algorithm(&self) -> &str {
        &self.algorithm
    }
    pub fn clear_algorithm(&mut self) {
        self.algorithm.clear();
    }

    // Param is passed by value, moved
    pub fn set_algorithm(&mut self, v: ::std::string::String) {
        self.algorithm = v;
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_algorithm(&mut self) -> &mut ::std::string::String {
        &mut self.algorithm
    }

    // Take field
    pub fn take_algorithm(&mut self) -> ::std::string::String {
        ::std::mem::replace(&mut self.algorithm, ::std::string::String::new())
    }

    // uint64 number_of_nodes = 2;


    pub fn get_number_of_nodes(&self) -> u64 {
        self.number_of_nodes
    }
    pub fn clear_number_of_nodes(&mut self) {
        self.number_of_nodes = 0;
    }

    // Param is passed by value, moved
    pub fn set_number_of_nodes(&mut self, v: u64) {
        self.number_of_nodes = v;
    }

    // uint64 number_of_proposals = 3;


    pub fn get_number_of_proposals(&self) -> u64 {
        self.number_of_proposals
    }
    pub fn clear_number_of_proposals(&mut self) {
        self.number_of_proposals = 0;
    }

    // Param is passed by value, moved
    pub fn set_number_of_proposals(&mut self, v: u64) {
        self.number_of_proposals = v;
    }

    // uint64 concurrent_proposals = 4;


    pub fn get_concurrent_proposals(&self) -> u64 {
        self.concurrent_proposals
    }
    pub fn clear_concurrent_proposals(&mut self) {
        self.concurrent_proposals = 0;
    }

    // Param is passed by value, moved
    pub fn set_concurrent_proposals(&mut self, v: u64) {
        self.concurrent_proposals = v;
    }

    // string reconfiguration = 5;


    pub fn get_reconfiguration(&self) -> &str {
        &self.reconfiguration
    }
    pub fn clear_reconfiguration(&mut self) {
        self.reconfiguration.clear();
    }

    // Param is passed by value, moved
    pub fn set_reconfiguration(&mut self, v: ::std::string::String) {
        self.reconfiguration = v;
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_reconfiguration(&mut self) -> &mut ::std::string::String {
        &mut self.reconfiguration
    }

    // Take field
    pub fn take_reconfiguration(&mut self) -> ::std::string::String {
        ::std::mem::replace(&mut self.reconfiguration, ::std::string::String::new())
    }

    // string reconfig_policy = 6;


    pub fn get_reconfig_policy(&self) -> &str {
        &self.reconfig_policy
    }
    pub fn clear_reconfig_policy(&mut self) {
        self.reconfig_policy.clear();
    }

    // Param is passed by value, moved
    pub fn set_reconfig_policy(&mut self, v: ::std::string::String) {
        self.reconfig_policy = v;
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_reconfig_policy(&mut self) -> &mut ::std::string::String {
        &mut self.reconfig_policy
    }

    // Take field
    pub fn take_reconfig_policy(&mut self) -> ::std::string::String {
        ::std::mem::replace(&mut self.reconfig_policy, ::std::string::String::new())
    }
}

impl ::protobuf::Message for AtomicBroadcastRequest {
    fn is_initialized(&self) -> bool {
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_singular_proto3_string_into(wire_type, is, &mut self.algorithm)?;
                },
                2 => {
                    if wire_type != ::protobuf::wire_format::WireTypeVarint {
                        return ::std::result::Result::Err(::protobuf::rt::unexpected_wire_type(wire_type));
                    }
                    let tmp = is.read_uint64()?;
                    self.number_of_nodes = tmp;
                },
                3 => {
                    if wire_type != ::protobuf::wire_format::WireTypeVarint {
                        return ::std::result::Result::Err(::protobuf::rt::unexpected_wire_type(wire_type));
                    }
                    let tmp = is.read_uint64()?;
                    self.number_of_proposals = tmp;
                },
                4 => {
                    if wire_type != ::protobuf::wire_format::WireTypeVarint {
                        return ::std::result::Result::Err(::protobuf::rt::unexpected_wire_type(wire_type));
                    }
                    let tmp = is.read_uint64()?;
                    self.concurrent_proposals = tmp;
                },
                5 => {
                    ::protobuf::rt::read_singular_proto3_string_into(wire_type, is, &mut self.reconfiguration)?;
                },
                6 => {
                    ::protobuf::rt::read_singular_proto3_string_into(wire_type, is, &mut self.reconfig_policy)?;
                },
                _ => {
                    ::protobuf::rt::read_unknown_or_skip_group(field_number, wire_type, is, self.mut_unknown_fields())?;
                },
            };
        }
        ::std::result::Result::Ok(())
    }

    // Compute sizes of nested messages
    #[allow(unused_variables)]
    fn compute_size(&self) -> u32 {
        let mut my_size = 0;
        if !self.algorithm.is_empty() {
            my_size += ::protobuf::rt::string_size(1, &self.algorithm);
        }
        if self.number_of_nodes != 0 {
            my_size += ::protobuf::rt::value_size(2, self.number_of_nodes, ::protobuf::wire_format::WireTypeVarint);
        }
        if self.number_of_proposals != 0 {
            my_size += ::protobuf::rt::value_size(3, self.number_of_proposals, ::protobuf::wire_format::WireTypeVarint);
        }
        if self.concurrent_proposals != 0 {
            my_size += ::protobuf::rt::value_size(4, self.concurrent_proposals, ::protobuf::wire_format::WireTypeVarint);
        }
        if !self.reconfiguration.is_empty() {
            my_size += ::protobuf::rt::string_size(5, &self.reconfiguration);
        }
        if !self.reconfig_policy.is_empty() {
            my_size += ::protobuf::rt::string_size(6, &self.reconfig_policy);
        }
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        if !self.algorithm.is_empty() {
            os.write_string(1, &self.algorithm)?;
        }
        if self.number_of_nodes != 0 {
            os.write_uint64(2, self.number_of_nodes)?;
        }
        if self.number_of_proposals != 0 {
            os.write_uint64(3, self.number_of_proposals)?;
        }
        if self.concurrent_proposals != 0 {
            os.write_uint64(4, self.concurrent_proposals)?;
        }
        if !self.reconfiguration.is_empty() {
            os.write_string(5, &self.reconfiguration)?;
        }
        if !self.reconfig_policy.is_empty() {
            os.write_string(6, &self.reconfig_policy)?;
        }
        os.write_unknown_fields(self.get_unknown_fields())?;
        ::std::result::Result::Ok(())
    }

    fn get_cached_size(&self) -> u32 {
        self.cached_size.get()
    }

    fn get_unknown_fields(&self) -> &::protobuf::UnknownFields {
        &self.unknown_fields
    }

    fn mut_unknown_fields(&mut self) -> &mut ::protobuf::UnknownFields {
        &mut self.unknown_fields
    }

    fn as_any(&self) -> &dyn (::std::any::Any) {
        self as &dyn (::std::any::Any)
    }
    fn as_any_mut(&mut self) -> &mut dyn (::std::any::Any) {
        self as &mut dyn (::std::any::Any)
    }
    fn into_any(self: Box<Self>) -> ::std::boxed::Box<dyn (::std::any::Any)> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        Self::descriptor_static()
    }

    fn new() -> AtomicBroadcastRequest {
        AtomicBroadcastRequest::new()
    }

    fn descriptor_static() -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "algorithm",
                    |m: &AtomicBroadcastRequest| { &m.algorithm },
                    |m: &mut AtomicBroadcastRequest| { &mut m.algorithm },
                ));
                fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeUint64>(
                    "number_of_nodes",
                    |m: &AtomicBroadcastRequest| { &m.number_of_nodes },
                    |m: &mut AtomicBroadcastRequest| { &mut m.number_of_nodes },
                ));
                fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeUint64>(
                    "number_of_proposals",
                    |m: &AtomicBroadcastRequest| { &m.number_of_proposals },
                    |m: &mut AtomicBroadcastRequest| { &mut m.number_of_proposals },
                ));
                fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeUint64>(
                    "concurrent_proposals",
                    |m: &AtomicBroadcastRequest| { &m.concurrent_proposals },
                    |m: &mut AtomicBroadcastRequest| { &mut m.concurrent_proposals },
                ));
                fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "reconfiguration",
                    |m: &AtomicBroadcastRequest| { &m.reconfiguration },
                    |m: &mut AtomicBroadcastRequest| { &mut m.reconfiguration },
                ));
                fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "reconfig_policy",
                    |m: &AtomicBroadcastRequest| { &m.reconfig_policy },
                    |m: &mut AtomicBroadcastRequest| { &mut m.reconfig_policy },
                ));
                ::protobuf::reflect::MessageDescriptor::new::<AtomicBroadcastRequest>(
                    "AtomicBroadcastRequest",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }

    fn default_instance() -> &'static AtomicBroadcastRequest {
        static mut instance: ::protobuf::lazy::Lazy<AtomicBroadcastRequest> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const AtomicBroadcastRequest,
        };
        unsafe {
            instance.get(AtomicBroadcastRequest::new)
        }
    }
}

impl ::protobuf::Clear for AtomicBroadcastRequest {
    fn clear(&mut self) {
        self.algorithm.clear();
        self.number_of_nodes = 0;
        self.number_of_proposals = 0;
        self.concurrent_proposals = 0;
        self.reconfiguration.clear();
        self.reconfig_policy.clear();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for AtomicBroadcastRequest {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for AtomicBroadcastRequest {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}
