//! Explicit, isolated 100k source measurement; not a claim of reference hardware qualification.
mod graph_support;
#[allow(dead_code)]
mod support;
use graph_support::*;
use opentrack_domain::*;
use opentrack_store as store;
use serde_json::json;
use support::Harness;
use uuid::Uuid;

async fn seed(h: &Harness, a: &Actor, w: Uuid, t: Uuid, start: i64, count: i64) {
    for offset in (0..count).step_by(500) {
        let mut tx = h.context(a.user.id, w).await;
        sqlx::query("SET LOCAL statement_timeout='120s'")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO issues(id,workspace_id,team_id,created_by_user_id,number,title,description,status_id,rank) SELECT gen_random_uuid(),$1,$2,$3,n,CASE WHEN n%100=0 THEN 'Rare quasar finding '||n ELSE 'Common nebula work '||n END,'Common nebula description with plain searchable text',s.id,n*1024 FROM generate_series($4,$5) n CROSS JOIN workflow_statuses s WHERE s.workspace_id=$1 AND s.team_id=$2 AND s.is_default")
        .bind(w).bind(t).bind(a.user.id).bind(start+offset).bind(start+offset+(count-offset).min(500)-1).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Explicit 100k load measurement; private PostgreSQL only"]
async fn search_at_100k_with_tenant_skew_and_permissions() {
    let h = Harness::new().await;
    let copy = h.clone();
    let result=tokio::spawn(async move {
        let h=&copy;let (a,w,t)=setup(h).await;let b=person(h,&a,w,"member").await;
        let private=team(h,&a,w,true).await;
        let wd=h.app.workspace(&a,w).await.unwrap();
        let second=h.app.create_workspace(&a,"load-workspace-1",req(json!({"organization_id":wd.organization_id,"name":"Skew tenant","team_name":"Skew team","team_key":"SKW"}))).await.unwrap();
        let st=h.app.bootstrap(&a,second.id).await.unwrap().teams[0].id;
        seed(h,&a,second.id,st,1,70000).await;
        seed(h,&a,w,private,1,27000).await;
        seed(h,&a,w,t,1,3000).await;
        sqlx::query("ANALYZE issues").execute(&h.owner).await.unwrap();
        sqlx::query("ANALYZE documents").execute(&h.owner).await.unwrap();
        let output=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/discovery-load");std::fs::create_dir_all(&output).unwrap();
        let mut results=Vec::new();
        for text in ["quasar","nebula","GRA-100","missingterm"] {
            let query:SearchRequest=req(json!({"query":{"version":1,"filter":null,"q":text},"types":["issue"]}));
            let mut times=Vec::new();let mut errors=0;
            for _ in 0..25 {
                let start=std::time::Instant::now();let page=h.app.search(&b,w,query.clone()).await;times.push(start.elapsed().as_secs_f64()*1000.);
                match page {Ok(page)=>{assert!(page.data.len()<=50);assert!(page.data.iter().all(|hit|hit.team_id==t));},Err(_)=>errors+=1}
            }
            times.sort_by(f64::total_cmp);let p95=times[23];
            let mut tx=store::begin(&h.app.pool).await.unwrap();store::context(&mut tx,b.user.id,Some(w)).await.unwrap();
            sqlx::query("SET LOCAL statement_timeout='5000ms'").execute(&mut *tx).await.unwrap();
            let plan=store::explain_search(&mut tx,w,b.user.id,&query,&IssueQueryContext{timezone:"UTC".into(),as_of:chrono::Utc::now()}).await.unwrap();
            tx.commit().await.unwrap();std::fs::write(output.join(format!("{text}-plan.json")),serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
            results.push(json!({"query":text,"runs":25,"p50_ms":times[12],"p95_ms":p95,"max_ms":times[24],"errors":errors}));
        }
        let receipt=json!({"sources":100000,"other_tenant":70000,"private_in_tenant":27000,"visible":3000,"samples":results});
        std::fs::write(output.join("summary.json"),serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();println!("{receipt}");
    }).await;
    h.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
