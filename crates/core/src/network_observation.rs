use serde::Serialize;
#[cfg(windows)]
use std::{mem::size_of, net::Ipv4Addr};

#[derive(Clone, Debug, Serialize)]
pub struct NetworkObservation {
    pub source: &'static str,
    pub interfaces: Vec<ObservedInterface>,
    pub default_gateways: Vec<ObservedGateway>,
    pub wan_visible_ip: ObservedValue,
    pub mappings: Vec<MappingObservation>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ObservedInterface {
    pub interface_index: u32,
    pub name: String,
    pub ipv4: Vec<String>,
    pub gateways: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ObservedGateway {
    pub interface_index: u32,
    pub address: String,
    pub source: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObservedValue {
    pub state: ObservationState,
    pub source: &'static str,
    pub value: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservationState {
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MappingMethod {
    UpnpIgd,
    Pcp,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MappingTransport {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MappingState {
    NotAttempted,
}

#[derive(Clone, Debug, Serialize)]
pub struct MappingObservation {
    pub method: MappingMethod,
    pub transport: MappingTransport,
    pub state: MappingState,
}

impl Default for ObservedValue {
    fn default() -> Self {
        Self {
            state: ObservationState::Unknown,
            source: "No external visible-IP observation is performed in M1",
            value: None,
        }
    }
}

impl Default for NetworkObservation {
    fn default() -> Self {
        Self {
            source: "UNKNOWN (Windows IP Helper observation unavailable)",
            interfaces: Vec::new(),
            default_gateways: Vec::new(),
            wan_visible_ip: ObservedValue::default(),
            mappings: [MappingMethod::UpnpIgd, MappingMethod::Pcp]
                .into_iter()
                .flat_map(|method| {
                    [MappingTransport::Tcp, MappingTransport::Udp].map(move |transport| {
                        MappingObservation {
                            method,
                            transport,
                            state: MappingState::NotAttempted,
                        }
                    })
                })
                .collect(),
        }
    }
}

/// Reads local IP Helper tables only. It does not contact a router or firewall.
pub fn observe_network() -> NetworkObservation {
    let mut result = NetworkObservation {
        source: "Windows IP Helper (GetAdaptersAddresses, GetIpForwardTable2)",
        ..Default::default()
    };

    #[cfg(not(windows))]
    {
        result.source = "UNKNOWN (Windows IP Helper is unavailable on this platform)";
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::ERROR_BUFFER_OVERFLOW,
            NetworkManagement::IpHelper::{
                FreeMibTable, GetAdaptersAddresses, GetIpForwardTable2, GAA_FLAG_INCLUDE_GATEWAYS,
                MIB_IPFORWARD_TABLE2,
            },
            Networking::WinSock::AF_INET,
        };

        let mut byte_count = 15 * 1024u32;
        let mut storage = vec![0usize; (byte_count as usize).div_ceil(size_of::<usize>())];
        // SAFETY: `storage` is a live, usize-aligned allocation whose byte
        // length is at least `byte_count`; the API writes only within that
        // caller-provided buffer and the reserved argument is null as required.
        let mut status = unsafe {
            GetAdaptersAddresses(
                AF_INET as u32,
                GAA_FLAG_INCLUDE_GATEWAYS,
                std::ptr::null(),
                storage.as_mut_ptr().cast(),
                &mut byte_count,
            )
        };
        if status == ERROR_BUFFER_OVERFLOW && byte_count <= 1024 * 1024 {
            storage.resize((byte_count as usize).div_ceil(size_of::<usize>()), 0);
            // SAFETY: resizing above preserves usize alignment and allocates
            // the byte count returned by IP Helper; that size is bounded to 1 MiB.
            status = unsafe {
                GetAdaptersAddresses(
                    AF_INET as u32,
                    GAA_FLAG_INCLUDE_GATEWAYS,
                    std::ptr::null(),
                    storage.as_mut_ptr().cast(),
                    &mut byte_count,
                )
            };
        }
        if status == 0 {
            // SAFETY: status 0 means IP Helper initialized the adapter list in
            // `storage`; all linked entries and their address lists remain valid
            // until this allocation is dropped below.
            unsafe { append_adapters(&mut result.interfaces, storage.as_mut_ptr().cast()) };
        } else {
            result.source = "Windows IP Helper (interface query unavailable)";
        }

        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        // SAFETY: `table` is a valid out pointer for GetIpForwardTable2.
        let route_status = unsafe { GetIpForwardTable2(AF_INET, &mut table) };
        if route_status == 0 && !table.is_null() {
            // SAFETY: this table came from the successful call above; its
            // NumEntries describes the flexible Table tail and it stays live
            // until the matching FreeMibTable call below.
            unsafe { append_default_routes(&mut result.default_gateways, table) };
            // SAFETY: this is the exact allocation returned on successful
            // GetIpForwardTable2 and has not been freed or retained elsewhere.
            unsafe { FreeMibTable(table.cast()) };
        }
    }
    result
}

/// Appends IPv4 interfaces from the buffer populated by IP Helper.
///
/// # Safety
/// `first` must be null or point to a complete `GetAdaptersAddresses` linked
/// list, with all entries, names, unicast addresses, and gateway addresses
/// valid for reads until this function returns.
#[cfg(windows)]
unsafe fn append_adapters(
    target: &mut Vec<ObservedInterface>,
    mut first: *mut windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH,
) {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        IP_ADAPTER_GATEWAY_ADDRESS_LH, IP_ADAPTER_UNICAST_ADDRESS_LH,
    };

    while !first.is_null() {
        // SAFETY: the caller guarantees every node in the API-provided linked
        // list is valid and remains within the live GetAdaptersAddresses buffer.
        let entry = unsafe { &*first };
        // SAFETY: Anonymous1 is a union returned initialized by IP Helper and
        // its Anonymous member exposes the documented IPv4 interface index.
        let index = unsafe { entry.Anonymous1.Anonymous.IfIndex };
        let name = if entry.FriendlyName.is_null() {
            String::new()
        } else {
            let mut len = 0;
            // SAFETY: FriendlyName is a valid NUL-terminated UTF-16 string in
            // the live IP Helper buffer; the scan stops at its terminator.
            while unsafe { *entry.FriendlyName.add(len) } != 0 {
                len += 1;
            }
            // SAFETY: the preceding scan established `len` readable UTF-16
            // code units before the terminator, all in the live buffer.
            let units = unsafe { std::slice::from_raw_parts(entry.FriendlyName, len) };
            String::from_utf16_lossy(units)
        };
        let mut interface = ObservedInterface {
            interface_index: index,
            name,
            ..Default::default()
        };
        // SAFETY: these address lists are linked from a valid adapter node in
        // the same live buffer and remain valid for the duration of this call.
        unsafe {
            read_sockaddr_list(
                entry
                    .FirstUnicastAddress
                    .cast::<IP_ADAPTER_UNICAST_ADDRESS_LH>(),
                |addr| interface.ipv4.push(addr),
            );
            read_sockaddr_list(
                entry
                    .FirstGatewayAddress
                    .cast::<IP_ADAPTER_GATEWAY_ADDRESS_LH>(),
                |addr| interface.gateways.push(addr),
            );
        }
        target.push(interface);
        first = entry.Next;
    }
}

/// Appends IPv4 default-route next hops from the IP Helper allocation.
///
/// # Safety
/// `table` must be the non-null table returned by a successful
/// `GetIpForwardTable2(AF_INET)` call and must remain allocated for this call.
#[cfg(windows)]
unsafe fn append_default_routes(
    target: &mut Vec<ObservedGateway>,
    table: *mut windows_sys::Win32::NetworkManagement::IpHelper::MIB_IPFORWARD_TABLE2,
) {
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    // SAFETY: the caller guarantees `table` is non-null and owned by the
    // successful GetIpForwardTable2 allocation.
    let count = unsafe { (*table).NumEntries as usize };
    // SAFETY: IP Helper allocates the flexible Table tail for NumEntries rows;
    // the returned allocation remains alive until its caller invokes FreeMibTable.
    let rows = unsafe { std::slice::from_raw_parts((*table).Table.as_ptr(), count) };
    for row in rows {
        if row.DestinationPrefix.PrefixLength == 0 {
            // SAFETY: NextHop is a union populated by IP Helper for AF_INET rows;
            // its Ipv4 member is the route's IPv4 next hop.
            let next_hop = unsafe { row.NextHop.Ipv4 };
            // SAFETY: IP Helper initializes the IN_ADDR union for this IPv4
            // route; reading its S_addr representation is valid for AF_INET.
            let address = unsafe { next_hop.sin_addr.S_un.S_addr };
            if next_hop.sin_family == AF_INET && address != 0 {
                let ip = Ipv4Addr::from(u32::from_be(address)).to_string();
                target.push(ObservedGateway {
                    interface_index: row.InterfaceIndex,
                    address: ip,
                    source: "GetIpForwardTable2 default route",
                });
            }
        }
    }
}

/// Walks an adapter address list and visits each IPv4 socket address.
///
/// # Safety
/// `node` must be null or a valid linked list of `T` nodes whose socket
/// addresses remain readable for the duration of the traversal.
#[cfg(windows)]
unsafe fn read_sockaddr_list<T>(mut node: *mut T, mut visit: impl FnMut(String))
where
    T: SockaddrNode,
{
    while !node.is_null() {
        // SAFETY: the function contract requires every non-null list node to
        // be valid until traversal completes.
        let item = unsafe { &*node };
        // SAFETY: the function contract guarantees the node's address storage
        // remains readable; the trait method validates the sockaddr shape.
        if let Some(ip) = unsafe { item.ipv4() } {
            visit(ip);
        }
        // SAFETY: the list node is valid and its Next pointer is part of that node.
        node = unsafe { item.next() };
    }
}

#[cfg(windows)]
trait SockaddrNode {
    /// Returns the next node in the IP Helper linked list.
    ///
    /// # Safety
    /// `self` must refer to a live node returned by IP Helper.
    unsafe fn next(&self) -> *mut Self;

    /// Reads this node's socket address if it represents an IPv4 sockaddr.
    ///
    /// # Safety
    /// `self` and its socket-address storage must be live and readable as
    /// reported by the IP Helper node.
    unsafe fn ipv4(&self) -> Option<String>;
}

/// Converts a validated socket-address pointer to an IPv4 string.
///
/// # Safety
/// If non-null, `address` must be readable for `length` bytes and point to a
/// Windows `SOCKADDR` supplied by a live IP Helper address node.
#[cfg(windows)]
unsafe fn socket_address_ipv4(
    address: *mut windows_sys::Win32::Networking::WinSock::SOCKADDR,
    length: i32,
) -> Option<String> {
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    if address.is_null() || length < size_of::<SOCKADDR_IN>() as i32 {
        return None;
    }
    // SAFETY: the caller contract guarantees a readable sockaddr for `length`
    // bytes; the size check above proves enough storage for SOCKADDR_IN.
    if unsafe { (*address).sa_family } != AF_INET {
        return None;
    }
    // SAFETY: the family and length checks establish that this is a complete
    // IPv4 SOCKADDR_IN before casting and reading its address union.
    let address = unsafe { &*address.cast::<SOCKADDR_IN>() };
    // SAFETY: the checked SOCKADDR_IN has AF_INET family, so its IN_ADDR S_addr
    // union field contains the initialized IPv4 address representation.
    let ipv4 = unsafe { address.sin_addr.S_un.S_addr };
    Some(Ipv4Addr::from(u32::from_be(ipv4)).to_string())
}

#[cfg(windows)]
impl SockaddrNode
    for windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_UNICAST_ADDRESS_LH
{
    /// # Safety
    /// `self` must be a live node returned by `GetAdaptersAddresses`.
    unsafe fn next(&self) -> *mut Self {
        self.Next
    }

    /// # Safety
    /// `self` and its sockaddr storage must remain valid in the IP Helper buffer.
    unsafe fn ipv4(&self) -> Option<String> {
        // SAFETY: caller-provided list membership guarantees this node and its
        // reported socket-address storage remain valid until conversion returns.
        unsafe { socket_address_ipv4(self.Address.lpSockaddr, self.Address.iSockaddrLength) }
    }
}

#[cfg(windows)]
impl SockaddrNode
    for windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_GATEWAY_ADDRESS_LH
{
    /// # Safety
    /// `self` must be a live node returned by `GetAdaptersAddresses`.
    unsafe fn next(&self) -> *mut Self {
        self.Next
    }

    /// # Safety
    /// `self` and its sockaddr storage must remain valid in the IP Helper buffer.
    unsafe fn ipv4(&self) -> Option<String> {
        // SAFETY: caller-provided list membership guarantees this node and its
        // reported socket-address storage remain valid until conversion returns.
        unsafe { socket_address_ipv4(self.Address.lpSockaddr, self.Address.iSockaddrLength) }
    }
}
