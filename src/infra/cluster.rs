// Copyright 2022 Zinc Labs Inc. and Contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use dashmap::DashMap;
use etcd_client::PutOptions;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use super::config::CONFIG;
use super::db::ETCD_CLIENT;
use super::errors::{Error, Result};
use crate::common::json;
use crate::infra::db::{etcd, Event};
use crate::service::db;

pub static mut LOCAL_NODE_ID: i32 = 0;
pub static mut LOCAL_NODE_KEY_LEASE_ID: i64 = 0;
pub static LOCAL_NODE_KEY_TTL: i64 = 10; // node ttl, seconds
pub static mut LOCAL_NODE_STATUS: NodeStatus = NodeStatus::Prepare;

lazy_static! {
    pub static ref LOCAL_NODE_UUID: String = load_local_node_uuid();
    pub static ref LOCAL_NODE_ROLE: Vec<Role> = load_local_node_role();
    pub static ref NODES: DashMap<String, Node> = DashMap::new();
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: i32,
    pub uuid: String,
    pub name: String,
    pub http_addr: String,
    pub grpc_addr: String,
    pub role: Vec<Role>,
    pub cpu_num: u64,
    pub status: NodeStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeStatus {
    Prepare,
    Online,
    Offline,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Role {
    All,
    Ingester,
    Querier,
    Compactor,
    Router,
    AlertManager,
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let s = s.to_lowercase();
        match s.as_str() {
            "all" => Ok(Role::All),
            "ingester" => Ok(Role::Ingester),
            "querier" => Ok(Role::Querier),
            "compactor" => Ok(Role::Compactor),
            "router" => Ok(Role::Router),
            "alertmanager" => Ok(Role::AlertManager),
            _ => Err(format!("Invalid cluster role: {}", s)),
        }
    }
}
impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::All => write!(f, "all"),
            Role::Ingester => write!(f, "ingester"),
            Role::Querier => write!(f, "querier"),
            Role::Compactor => write!(f, "compactor"),
            Role::Router => write!(f, "router"),
            Role::AlertManager => write!(f, "alertmanager"),
        }
    }
}

/// Register and keepalive the node to cluster
pub async fn register_and_keepalive() -> Result<()> {
    if CONFIG.common.local_mode {
        let nodes = load_local_node_role();
        if !is_single_node(nodes) {
            panic!("For local mode only single node deployment is allowed !");
        }
        return Ok(());
    }
    if let Err(e) = register().await {
        log::error!("Register to cluster failed: {}", e);
        return Err(e);
    }

    // keep alive
    tokio::task::spawn(async move {
        loop {
            if is_offline() {
                break;
            }
            let lease_id = unsafe { LOCAL_NODE_KEY_LEASE_ID };
            let ret = etcd::keepalive_lease_id(lease_id, LOCAL_NODE_KEY_TTL, is_offline).await;
            if ret.is_ok() {
                break;
            }
            let e = ret.unwrap_err();
            let estr = e.to_string();
            if is_offline()
                || estr
                    != Error::from(etcd_client::Error::LeaseKeepAliveError(
                        "lease expired or revoked".to_string(),
                    ))
                    .to_string()
            {
                break;
            }
            log::error!("keepalive lease id expired or revoked, set node online again.");
            // get new lease id
            let mut client = ETCD_CLIENT.get().await.clone().unwrap();
            let resp = match client.lease_grant(LOCAL_NODE_KEY_TTL, None).await {
                Ok(resp) => resp,
                Err(e) => {
                    log::error!("lease grant failed: {}", e);
                    continue;
                }
            };
            let id = resp.id();
            // update local node key lease id
            unsafe {
                LOCAL_NODE_KEY_LEASE_ID = id;
            }
            if let Err(e) = set_online().await {
                log::error!("set node online failed: {}", e);
                continue;
            }
        }
    });

    Ok(())
}

/// Register to cluster
pub async fn register() -> Result<()> {
    // 1. create a cluster lock for node register
    let mut locker = etcd::Locker::new("nodes/register");
    locker.lock(0).await?;

    // 2. get node list
    let node_list = list_nodes().await?;

    // 3. calculate node_id
    let mut node_id = 1;
    let mut node_ids = Vec::new();
    for node in node_list {
        node_ids.push(node.id);
        NODES.insert(node.uuid.clone(), node);
    }
    node_ids.sort();
    for id in &node_ids {
        if *id == node_id {
            node_id += 1;
        } else {
            break;
        }
    }
    // update local id
    unsafe {
        LOCAL_NODE_ID = node_id;
    }

    // 4. join the cluster
    let key = format!("{}nodes/{}", &CONFIG.etcd.prefix, *LOCAL_NODE_UUID);
    let val = Node {
        id: node_id,
        uuid: LOCAL_NODE_UUID.clone(),
        name: CONFIG.common.instance_name.clone(),
        http_addr: format!("http://{}:{}", get_node_ip(), CONFIG.http.port),
        grpc_addr: format!("http://{}:{}", get_node_ip(), CONFIG.grpc.port),
        role: LOCAL_NODE_ROLE.clone(),
        cpu_num: sys_info::cpu_num().unwrap() as u64,
        status: NodeStatus::Prepare,
    };
    // cache local node
    NODES.insert(LOCAL_NODE_UUID.clone(), val.clone());
    let val = json::to_string(&val).unwrap();
    // register node to cluster
    let mut client = ETCD_CLIENT.get().await.clone().unwrap();
    let resp = client.lease_grant(LOCAL_NODE_KEY_TTL, None).await?;
    let id = resp.id();
    // update local node key lease id
    unsafe {
        LOCAL_NODE_KEY_LEASE_ID = id;
    }
    let opt = PutOptions::new().with_lease(id);
    let _resp = client.put(key, val, Some(opt)).await?;

    // 5. watch node list
    tokio::task::spawn(async move { watch_node_list().await });

    // 7. register ok, release lock
    locker.unlock().await?;

    log::info!("[TRACE] Register to cluster ok");
    Ok(())
}

/// set online to cluster
pub async fn set_online() -> Result<()> {
    if CONFIG.common.local_mode {
        return Ok(());
    }

    // set node status to online
    let val = match NODES.get(LOCAL_NODE_UUID.as_str()) {
        Some(node) => {
            let mut val = node.value().clone();
            val.status = NodeStatus::Online;
            val
        }
        None => Node {
            id: unsafe { LOCAL_NODE_ID },
            uuid: LOCAL_NODE_UUID.clone(),
            name: CONFIG.common.instance_name.clone(),
            http_addr: format!("http://{}:{}", get_node_ip(), CONFIG.http.port),
            grpc_addr: format!("http://{}:{}", get_node_ip(), CONFIG.grpc.port),
            role: LOCAL_NODE_ROLE.clone(),
            cpu_num: sys_info::cpu_num().unwrap() as u64,
            status: NodeStatus::Online,
        },
    };

    unsafe {
        LOCAL_NODE_STATUS = NodeStatus::Online;
    }

    // cache local node
    NODES.insert(LOCAL_NODE_UUID.clone(), val.clone());
    let val = json::to_string(&val).unwrap();

    let mut client = ETCD_CLIENT.get().await.clone().unwrap();
    let key = format!("{}nodes/{}", &CONFIG.etcd.prefix, *LOCAL_NODE_UUID);
    let opt = PutOptions::new().with_lease(unsafe { LOCAL_NODE_KEY_LEASE_ID });
    let _resp = client.put(key, val, Some(opt)).await?;

    Ok(())
}

/// Leave cluster
pub async fn leave() -> Result<()> {
    if CONFIG.common.local_mode {
        return Ok(());
    }

    unsafe {
        LOCAL_NODE_STATUS = NodeStatus::Offline;
    }

    let mut client = ETCD_CLIENT.get().await.clone().unwrap();
    let key = format!("{}nodes/{}", &CONFIG.etcd.prefix, *LOCAL_NODE_UUID);
    let _resp = client.delete(key, None).await?;

    Ok(())
}

#[inline(always)]
pub fn get_cached_online_nodes() -> Option<Vec<Node>> {
    if NODES.len() == 0 {
        return None;
    }
    let mut node_list = Vec::new();
    for node in NODES.iter() {
        if node.status.eq(&NodeStatus::Online) {
            node_list.push(node.value().clone());
        }
    }
    Some(node_list)
}

#[inline(always)]
pub fn get_cached_online_query_nodes() -> Option<Vec<Node>> {
    if NODES.len() == 0 {
        return None;
    }
    let mut node_list = Vec::new();
    for node in NODES.iter() {
        if node.status.eq(&NodeStatus::Online)
            && (is_querier(&node.value().role) || is_ingester(&node.value().role))
        {
            node_list.push(node.value().clone());
        }
    }
    Some(node_list)
}

#[inline]
pub fn get_cached_online_ingester_nodes() -> Option<Vec<Node>> {
    if NODES.len() == 0 {
        return None;
    }
    let mut node_list = Vec::new();
    for node in NODES.iter() {
        if node.status.eq(&NodeStatus::Online) && is_ingester(&node.value().role) {
            node_list.push(node.value().clone());
        }
    }
    Some(node_list)
}

#[inline]
pub fn get_cached_online_querier_nodes() -> Option<Vec<Node>> {
    if NODES.len() == 0 {
        return None;
    }
    let mut node_list = Vec::new();
    for node in NODES.iter() {
        if node.status.eq(&NodeStatus::Online) && is_querier(&node.value().role) {
            node_list.push(node.value().clone());
        }
    }
    Some(node_list)
}

/// List nodes from cluster or local cache
async fn list_nodes() -> Result<Vec<Node>> {
    let mut nodes = Vec::new();
    let mut client = ETCD_CLIENT.get().await.clone().unwrap();
    let key = format!("{}nodes/", &CONFIG.etcd.prefix);
    let opt = etcd_client::GetOptions::new().with_prefix();
    let ret = client.get(key.clone(), Some(opt)).await.map_err(|e| {
        log::error!("Error getting node list from etcd {}: {}", key.clone(), e);
        e
    })?;

    for item in ret.kvs() {
        // let item_key = item.key_str().unwrap().to_string();
        // let short_key = item_key.strip_prefix(&format!("{}nodes/", &CONFIG.etcd.prefix)).unwrap();
        // println!("key: {:?}", item_key);
        // println!("key: {:?}", short_key);
        // println!("val: {:?}", item.value_str());
        let node: Node = json::from_slice(item.value())?;
        nodes.push(node.to_owned());
    }

    Ok(nodes)
}

async fn watch_node_list() -> Result<()> {
    let db = &super::db::DEFAULT;
    let key = "/nodes/";
    let mut events = db.watch(key).await?;
    let events = Arc::get_mut(&mut events).unwrap();
    log::info!("[TRACE] Start watching node_list");
    loop {
        let ev = match events.recv().await {
            Some(ev) => ev,
            None => {
                log::error!("watch_node_list: event channel closed");
                break;
            }
        };
        match ev {
            Event::Put(ev) => {
                let item_key = ev.key.strip_prefix(key).unwrap();
                let item_value: Node = json::from_slice(&ev.value.unwrap()).unwrap();
                log::info!("[TRACE] cluster->node: join {:?}", item_value.clone());
                NODES.insert(item_key.to_string(), item_value.clone());
                // need broadcast local file list
                if item_value.status.eq(&NodeStatus::Online)
                    && !LOCAL_NODE_UUID.eq(item_key)
                    && is_ingester(&LOCAL_NODE_ROLE)
                {
                    db::file_list::local::broadcast_cache().await.unwrap();
                }
            }
            Event::Delete(ev) => {
                let item_key = ev.key.strip_prefix(key).unwrap();
                let item_value = NODES.get(item_key).unwrap().clone();
                log::info!("[TRACE] cluster->node: leave {:?}", item_value.clone());
                NODES.remove(item_key);
            }
        }
    }

    Ok(())
}

#[inline(always)]
fn load_local_node_uuid() -> String {
    Uuid::new_v4().to_string()
}

#[inline(always)]
pub fn load_local_node_role() -> Vec<Role> {
    CONFIG
        .common
        .node_role
        .clone()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect::<Vec<Role>>()
}

#[inline(always)]
pub fn is_ingester(role: &[Role]) -> bool {
    if role.contains(&Role::All) {
        return true;
    }
    if role.contains(&Role::Ingester) {
        return true;
    }
    false
}

#[inline(always)]
pub fn is_querier(role: &[Role]) -> bool {
    if role.contains(&Role::All) {
        return true;
    }
    if role.contains(&Role::Querier) {
        return true;
    }
    false
}

#[inline(always)]
pub fn is_compactor(role: &[Role]) -> bool {
    role.contains(&Role::All) || role.contains(&Role::Compactor)
}

#[inline(always)]
pub fn is_router(role: &[Role]) -> bool {
    role.contains(&Role::Router)
}

#[inline(always)]
pub fn is_alert_manager(role: &[Role]) -> bool {
    role.contains(&Role::All) || role.contains(&Role::AlertManager)
}

#[inline(always)]
pub fn is_single_node(role: Vec<Role>) -> bool {
    role.contains(&Role::All)
}

#[inline(always)]
pub fn is_offline() -> bool {
    unsafe { LOCAL_NODE_STATUS == NodeStatus::Offline }
}

#[inline(always)]
pub fn ge_node_by_uuid(uuid: &str) -> Option<Node> {
    NODES.get(uuid).map(|node| node.clone())
}

#[inline(always)]
pub fn get_node_ip() -> String {
    for adapter in get_if_addrs::get_if_addrs().unwrap() {
        if adapter.is_loopback() {
            continue;
        }
        match adapter.ip() {
            IpAddr::V4(_) => return adapter.ip().to_string(),
            IpAddr::V6(_) => (),
        };
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_role() {
        let parse = |s: &str| s.parse::<Role>().unwrap();

        assert_eq!(parse("all"), Role::All);
        assert_eq!(parse("ingester"), Role::Ingester);
        assert_eq!(parse("querier"), Role::Querier);
        assert_eq!(parse("compactor"), Role::Compactor);
        assert_eq!(parse("router"), Role::Router);
        assert_eq!(parse("alertmanager"), Role::AlertManager);
        assert_eq!(parse("alertManager"), Role::AlertManager);
        assert_eq!(parse("AlertManager"), Role::AlertManager);
        assert!("alert_manager".parse::<Role>().is_err());
    }

    #[test]
    fn test_is_querier() {
        assert!(is_querier(&[Role::Querier]));
        assert!(is_querier(&[Role::All]));
        assert!(!is_querier(&[Role::Ingester]));
    }

    #[test]
    fn test_is_ingester() {
        assert!(is_ingester(&[Role::Ingester]));
        assert!(is_ingester(&[Role::All]));
        assert!(!is_ingester(&[Role::Querier]));
    }

    #[test]
    fn test_is_compactor() {
        assert!(is_compactor(&[Role::Compactor]));
        assert!(is_compactor(&[Role::All]));
        assert!(!is_compactor(&[Role::Querier]));
    }

    #[test]
    fn test_is_router() {
        assert!(is_router(&[Role::Router]));
        assert!(!is_router(&[Role::All]));
        assert!(!is_router(&[Role::Querier]));
    }

    #[test]
    fn test_is_alert_manager() {
        assert!(is_alert_manager(&[Role::AlertManager]));
        assert!(is_alert_manager(&[Role::All]));
        assert!(!is_alert_manager(&[Role::Querier]));
    }

    #[test]
    fn test_load_local_node_uuid() {
        assert!(!load_local_node_uuid().is_empty());
    }

    #[actix_web::test]
    #[ignore]
    async fn test_list_nodes() {
        assert!(list_nodes().await.unwrap().is_empty());
    }

    #[actix_web::test]
    async fn test_cluster() {
        register_and_keepalive().await.unwrap();
        set_online().await.unwrap();
        leave().await.unwrap();
        assert!(get_cached_online_nodes().is_none());
        assert!(get_cached_online_query_nodes().is_none());
        assert!(get_cached_online_ingester_nodes().is_none());
        assert!(get_cached_online_querier_nodes().is_none());
        assert!(get_cached_online_querier_nodes().is_none());
    }

    #[test]
    fn test_get_node_ip() {
        assert!(!get_node_ip().is_empty());
    }
}
