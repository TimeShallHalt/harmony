// Isolated fixture contract check for the Android server, group and thread flows.
// Run from the project root with bun android/testing/conversation_fixture.ts.
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../",import.meta.url));
const reserve = createServer();
await new Promise<void>(resolve => reserve.listen(0,"127.0.0.1",resolve));
const address = reserve.address();
assert(address && typeof address === "object");
const port = address.port;
await new Promise<void>(resolve => reserve.close(() => resolve()));
const origin = "http://127.0.0.1:" + port;
const token = "yapfast-test-token";
const app = Bun.spawn([process.execPath,"testing/fake_discord.ts","--port",String(port)],{cwd:root,stdout:"ignore",stderr:"pipe"});
const sockets: WebSocket[] = [];
const guild = "1400000000000000100", general = "1400000000000000102";
const friends = ["1400000000000000002","1400000000000000003"];

async function eventually<T>(value: () => T | Promise<T>): Promise<NonNullable<T>> {
    const limit=Date.now()+10_000;
    do {
        const result=await value();
        if (result) return result as NonNullable<T>;
        await Bun.sleep(40);
    } while(Date.now()<limit);
    throw Error("The fixture did not produce the expected event within ten seconds.");
}

async function api(method: string,path: string,body?: object,headers?: Record<string,string>) {
    const response=await fetch(origin+"/api/v9"+path,{
        method,headers:{Authorization:token,"Content-Type":"application/json",...headers},
        body:body && JSON.stringify(body),
    });
    assert(response.ok,method+" "+path+" returned "+response.status);
    return response.json();
}

function connect() {
    const socket=new WebSocket(origin.replace("http:","ws:")+"/?encoding=json&v=9");
    sockets.push(socket);
    const received: { t: string, d: any }[]=[];
    socket.onmessage = e => {
        const frame=JSON.parse(String(e.data));
        if(frame.op===10) socket.send(JSON.stringify({op:2,d:{token}}));
        if(frame.t) received.push(frame);
    };
    return {socket,event:(name:string, match:(d:any)=>boolean=()=>true)=>
        eventually(()=>received.find(f=>f.t===name && match(f.d))?.d)};
}

try {
    await eventually(async()=> {
        try{return (await fetch(origin+"/api/v9/users/@me",{headers:{Authorization:token}})).ok;}catch{return false;}
    });
    const client=connect();
    const ready=await client.event("READY");
    const invite=await api("GET","/invites/server?with_counts=true");
    assert.equal(invite.guild.id,guild);
    const context=btoa(JSON.stringify({location:"Join Guild",location_guild_id:guild,location_channel_id:general,location_channel_type:0}));
    const joined=await api("POST","/invites/server",{session_id:ready.session_id},{"X-Context-Properties":context});
    assert.equal(joined.guild.id,guild);
    const refused=await fetch(origin+"/api/v9/invites/unknown-in-android-check",{headers:{Authorization:token}});
    assert.equal(refused.status,404);

    const group=await api("POST","/users/@me/channels",{recipients:friends});
    assert.equal(group.type,3);
    assert.equal(group.owner_id,ready.user.id);
    await client.event("CHANNEL_CREATE",d=>d.id===group.id);
    const renamed=await api("PATCH","/channels/"+group.id,{name:"Fixture group"});
    assert.equal(renamed.name,"Fixture group");
    await client.event("CHANNEL_UPDATE",d=>d.id===group.id && d.name==="Fixture group");
    const sent=await api("POST","/channels/"+group.id+"/messages",{content:"Group delivery",nonce:"281001"});
    assert.equal(sent.content,"Group delivery");
    await api("POST","/channels/"+group.id+"/messages/"+sent.id+"/ack",{token:null});
    const receipts=await (await fetch(origin+"/_control/acks")).json();
    assert.deepEqual(receipts,[[group.id]]);
    await api("POST","/channels/"+group.id+"/messages/0/ack",{manual:true, mention_count:0, token:null});
    const manual=await (await fetch(origin+"/_control/manual-acks")).json();
    assert.equal(manual.length,1);
    assert.equal(manual[0].channel_id,group.id);

    const created=await api("POST","/guilds",{name:"Fixture studio",guild_template_code:"2TffvPucqHkN",channels:[],system_channel_id:null,icon:null});
    assert.equal(created.owner_id,ready.user.id);
    const newGuild=await client.event("GUILD_CREATE",d=>d.id===created.id);
    assert.equal(newGuild.properties.name,"Fixture studio");
    const room=newGuild.channels.find((r:any)=>r.name==="general");
    assert(room);
    const post=await api("POST","/channels/"+room.id+"/messages",{content:"First studio message",nonce:"281002"});
    assert.equal(post.content,"First studio message");
    assert.equal((await client.event("MESSAGE_CREATE",d=>d.id===post.id)).guild_id,created.id);

    client.socket.send(JSON.stringify({op:37,d:{subscriptions:{[guild]:{threads:true,channels:{[general]:[[0,99]]}}}}}));
    const active=await client.event("THREAD_LIST_SYNC",d=>d.guild_id===guild);
    assert(active.threads.some((t:any)=>t.name==="Book club"));
    assert(!active.threads.some((t:any)=>t.name==="Surprise for Bob"));
    const roster=await client.event("GUILD_MEMBER_LIST_UPDATE",d=>d.guild_id===guild);
    assert(roster.ops.some((op:any)=>op.items?.some((row:any)=>row.member?.user?.username==="bob")));
    const archives=async(offset:number)=>eventually(async()=>{
        const page=await api("GET","/channels/"+general+"/threads/search?archived=true&limit=25&offset="+offset);
        return Array.isArray(page.threads) && page;
    });
    const first=await archives(0),last=await archives(25);
    assert.equal(first.threads.length,25);
    assert.equal(first.has_more,true);
    assert.equal(last.threads.length,5);

    const next=connect();
    const reloaded=await next.event("READY");
    assert(reloaded.guilds.some((g:any)=>g.id===created.id));
    assert(reloaded.private_channels.some((c:any)=>c.id===group.id && c.name==="Fixture group"));
    console.log("Passed invitation review and acceptance, invalid invite, group creation, rename and single-channel read receipts, server creation, message delivery, gateway member and thread permissions, archive pagination, and reconnect.");
} finally {
    for(const ws of sockets) ws.close();
    app.kill();
    await app.exited;
}
