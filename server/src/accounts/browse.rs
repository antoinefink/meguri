// SPDX-License-Identifier: MIT
//! Browser-session adapter for the shared, synchronous note transactions.
use super::*;
use crate::collection::{mcp::{self, browse, Authority, Backend, Context}, session::Sessions};
use crate::storage::collection::ReadPin;
use std::{path::{Path, PathBuf}, time::Duration};

#[derive(Serialize)]
struct Catalog { generation:String, #[serde(flatten)] catalog:browse::Catalog }

pub(crate) enum Operation {
    Catalog(mcp::CatalogInput), Page(browse::PageInput), Detail(browse::DetailInput),
    Create(browse::Create), Edit(mcp::EditInput), Delete(browse::Delete),
}
impl Service {
    pub(crate) async fn browse(&self, original:BrowserAuthority, expected:String, op:Operation) -> Result<serde_json::Value> {
        let actor=original.clone();
        // Resolve and validate identity before queueing. No OAuth exchange and
        // no caller-supplied identifier ever selects an account or file.
        let epoch=self.metadata.run(move |c| {
            admission(c)?;
            let account=check_browser(c,&actor,"authentication_failed")?;
            if expected!=account.identity.id{return Err(Error::new("stale_authority"));}
            crate::storage::reset::admitted_epoch(c,&account.identity.id)
        }).await?;
        let id=original.identity().id.clone();
        let gate=self.barriers.get(&id)?;
        let queue=tokio::time::timeout(Duration::from_secs(6),gate.enter_mcp()).await.map_err(|_|mcp::busy())?;
        let write=matches!(&op,Operation::Create(_)|Operation::Edit(_)|Operation::Delete(_));
        let guard=if write {gate.try_enter().ok_or_else(mcp::busy)?} else {
            tokio::time::timeout(Duration::from_millis(500),gate.enter()).await.map_err(|_|mcp::busy())?
        };
        let browser_commit=if write {Some(gate.try_browser_commit().ok_or_else(mcp::busy)?)} else {None};
        let root=self.metadata.owner();
        let sessions=self.normal_sessions.clone();
        let key=self.mcp_key;
        #[cfg(feature="test-hooks")]
        let hooks=self.hooks.clone();
        // Capacity acquisition is try-only; all guards then move into the
        // durable worker, including when the HTTP caller goes away.
        self.metadata.files_try(move |_| {
            let _held=(gate,guard,queue,browser_commit);
            check(&root,&original,epoch)?;
            let path=root.collection_path(&id)?;
            let pin=root.pin_collection(&path)?;
            let publication=path.parent().and_then(Path::file_name).and_then(|s|s.to_str()).ok_or_else(||Error::new("storage_unavailable"))?;
            let context=Context {account:id.clone(),publication:format!("{}:{epoch}:{publication}",original.account.generation)};
            let a=BrowserCollection {original,epoch,context,root,path:path.clone(),sessions,write,_pin:pin,
                #[cfg(feature="test-hooks")]
                hooks,
            };
            let backend=Backend::new(key);
            let value=match op {
                Operation::Catalog(input)=>serde_json::to_value(Catalog {generation:backend.browse_generation(&a)?,catalog:backend.browse_catalog(&path,&a,input)?}),
                Operation::Page(input)=>serde_json::to_value(backend.browse_page(&path,&a,input)?),
                Operation::Detail(input)=>serde_json::to_value(backend.browse_detail(&path,&a,input)?),
                Operation::Create(input)=>{
                    backend.check_browse_generation(&a,&input.generation)?;
                    serde_json::to_value(backend.create_note(&path,&a,mcp::CreateInput {deck_id:input.deck_id,note_type_id:input.note_type_id,fields:input.fields,tags:input.tags},&crate::mcp_service::Runtime)?)
                },
                Operation::Edit(input)=>serde_json::to_value(backend.edit_note(&path,&a,input,&crate::mcp_service::Runtime)?),
                Operation::Delete(input)=>{backend.browse_delete(&path,&a,input,&crate::mcp_service::Runtime)?;Ok(serde_json::json!({}))},
            }.map_err(|_|Error::new("internal_error"))?;
            let mut value=value;
            value.as_object_mut().ok_or_else(||Error::new("internal_error"))?.insert("account_id".into(),id.into());
            Ok(value)
        }).await.map_err(browser_error)
    }
}
fn browser_error(e:Error) -> Error {
    match e.code {
        "invalid_input"=>Error::new("invalid_request"),
        "not_found"=>Error::new("route_not_found"),
        "conflict"=>Error::new("browse_conflict"),
        "result_too_large"=>Error::new("request_too_large"),
        "unsupported_collection"=>Error::new("storage_unavailable"),
        _=>e,
    }
}
fn check(root:&DataRoot,original:&BrowserAuthority,epoch:i64) -> Result<()> {
    let mut c=crate::storage::inspect(root.database())?;
    c.busy_timeout(Duration::ZERO)?;
    let tx=c.transaction()?;
    admission(&tx)?;
    check_browser(&tx,original,"authentication_failed")?;
    if crate::storage::reset::admitted_epoch(&tx,&original.identity().id)?!=epoch {return Err(Error::new("browse_conflict"));}
    tx.commit()?;Ok(())
}
struct BrowserCollection {
    original:BrowserAuthority, epoch:i64, context:Context, root:Arc<DataRoot>,path:PathBuf,
    sessions:Arc<Sessions>,write:bool,_pin:ReadPin,
    #[cfg(feature="test-hooks")]
    hooks:Arc<crate::test_hooks::Hooks>,
}
impl Authority for BrowserCollection {
    type Guard=();
    fn context(&self)->&Context {&self.context}
    fn admit(&self,write:bool)->Result<()> {
        if write!=self.write {return Err(Error::new("internal_error"));}
        self.validate()?;
        if write && self.sessions.has_retained_writer(&self.original.identity().id)? {return Err(mcp::busy());}
        Ok(())
    }
    fn recheck(&self,_:&())->Result<()> {
        #[cfg(feature="test-hooks")]
        if self.write {tokio::runtime::Handle::current().block_on(self.hooks.hit("browse_before_completion"));}
        self.validate()
    }
}
impl BrowserCollection {
    fn validate(&self)->Result<()> {
        check(&self.root,&self.original,self.epoch)?;
        if self.root.collection_path(&self.original.identity().id)?!=self.path {return Err(Error::new("browse_conflict"));}
        Ok(())
    }
}
