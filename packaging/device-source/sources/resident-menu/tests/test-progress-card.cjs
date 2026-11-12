const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/../qml/NotificationsPage.qml`, 'utf8');
const shell = fs.readFileSync(`${__dirname}/../qml/shell.qml`, 'utf8');
const expression = source.match(/readonly property var liveTransactions: ([\s\S]*?)\n    readonly property var records:/)[1];
const functions = source.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
const jobId = 'a'.repeat(32), commands = [], opened = [], rows = [];
const state = {transactions:[{id:jobId,name:'Game',operation:'install',phase:'running',stage:'Installing',percent:21,created:100}],
  client:{records:[{id:'old',title:'Earlier',dismissed:false}],available:true,send:msg=>commands.push(msg)},
  selectedId:'', detail:null,clearIds:[],confirming:false,confirmSelected:0,visible:true,uiScale:1,
  Qt:{callLater:fn=>fn()},ListView:{Contain:0},history:{positionViewAtIndex:()=>{}},
  details:{contentHeight:200,height:100,contentY:0},openTransaction:id=>opened.push(id),
  visibleRecords:{get count(){return rows.length},get:i=>rows[i],remove:i=>rows.splice(i,1),
    insert:(i,row)=>rows.splice(i,0,row),move:(from,to)=>rows.splice(to,0,rows.splice(from,1)[0]),
    setProperty:(i,key,value)=>rows[i][key]=value}};
vm.createContext(state);
Object.defineProperties(state,{
  records:{get:()=>Array.from(vm.runInContext(expression,state)).concat(state.client.records.filter(record=>!record.dismissed))},
  selected:{get:()=>state.records.find(record=>record.id===state.selectedId)||null},
  blocked:{get:()=>state.confirming||state.detail!==null}
});
vm.runInContext(functions.join('\n'),state);
state.sync();
assert.equal(rows.length,2);assert.equal(state.selectedId,'transaction:'+jobId);
const firstRow=rows[0];
for(let percent=22;percent<90;percent++){
  state.transactions[0].percent=percent;state.sync();
  assert.equal(rows.length,2);assert.equal(rows[0],firstRow);assert.equal(state.selectedId,'transaction:'+jobId);
}
assert.equal(state.client.records.length,1,'percentage updates do not enter history');
state.dismiss();assert.equal(commands.length,0,'X cannot dismiss or cancel a live job');
state.activate();assert.deepEqual(opened,[jobId]);
state.clear();state.confirmSelected=1;state.activate();
assert.deepEqual(Array.from(commands[0].ids),['old'],'clear targets only archived history');
state.client.records=[];state.sync();assert.equal(rows.length,1,'live card survives history clear');
state.transactions[0].phase='succeeded';
state.client.records=[{id:'completed',title:'Installed',target:'dukkan-transaction:'+jobId}];
state.sync();assert.equal(rows.length,1);assert.equal(rows[0].recordId,'completed');
const iconExpression=shell.match(/name: (menuRow\.modelData\.key === "resume"[^\n]+)/)[1];
for(const [key,id,foreground,expected] of [
  ['resume','dukkan','storefront','store-dukkan'],['resume','steam','storefront','store-steam'],
  ['resume','','library','library'],['resume','','game','resume'],['store-steam','','library','store-steam']]){
  assert.equal(vm.runInNewContext(iconExpression,{menuRow:{modelData:{key}},root:{storefrontId:id,foreground}}),expected);
}


state.transactions=[];state.client.records=[];state.detail={id:'stale'};state.sync();
assert.equal(rows.length,0);assert.equal(state.selectedId,'');assert.equal(state.detail,null);
assert.match(source,/visible: page.hasItems && !page.blocked/);
assert.match(source,/visible: !page.hasItems && !page.blocked/);
const guide=shell.slice(shell.indexOf('id: navigationGuide'));
const legend=guide.match(/model: (\[[\s\S]*?\n\s*\])/)[1];
const entries=vm.runInNewContext('('+legend+')',{
 root:{page:'notifications'},notificationsPage:{hasItems:false,blocked:false,selected:null},
 notifications:{available:true,popups:[],records:[]},navigationGuide:{adjusting:false,textEntry:false}});
assert.deepEqual(Array.from(entries,e=>e.show!==false),[false,false,true,false,false], 'empty view offers Back only');
assert.doesNotMatch(shell,/MenuIcon \{ name: "store-dukkan"/,'Home category has no paired storefront emblems');
assert.match(source,/id: progressTrack/);
console.log('PASS: stable live progress card, history/dismissal isolation, transaction navigation and empty-state actions and Resume branding.');
