const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const path = require('node:path');
const {fixture} = require('./menu-controller-regression.cjs');
const s = fixture('notifications');
s.activeTab = 1;
s.openTransfers();
assert.equal(s.menuOpen, true);
assert.equal(s.page, 'transfer');
assert.equal(s.returnPage, 'notifications');
assert.equal(s.writes.length, 0, 'Opening the inbox keeps the current menu input lease');
s.back();
assert.equal(s.page, 'notifications');
assert.equal(s.menuOpen, true);
s.transferResult(JSON.stringify({type:'transfer',jobs:[{id:'pending'}],pending:1}));
assert.equal(s.transferAvailable, true);
assert.equal(s.transferSnapshot.jobs.length, 1);
const routing=fixture('transfer');
routing.notifications.available=false;
for (const event of ['ui_context','ui_action']) {
  routing.input(event,1);routing.input(event,1);routing.input(event,0);
}
assert.equal(routing.actions.join(','),'transfer-dismiss,transfer-clear','X/Y route to Transfer once per press, independently of notification availability');
routing.page='notifications';routing.notifications.available=true;
routing.input('ui_context',1);routing.input('ui_context',0);
routing.input('ui_action',1);routing.input('ui_action',0);
assert.equal(routing.actions.slice(-2).join(','),'dismiss,clear','Notifications keep the same controls');
routing.page='';routing.notifications.popups=[{id:'popup'}];routing.notificationShortcut(false);
assert.equal(routing.actions.pop().action,'dismiss-visible','Other pages keep popup dismissal');
const shellSource=fs.readFileSync(path.join(__dirname,'../qml/shell.qml'),'utf8');
const guideExpression='['+shellSource.split('id: navigationGuide')[1].split('model: [')[1].split('\n                        ]')[0]+']';
const footerContext={root:{page:'transfer'},navigationGuide:{adjusting:false,textEntry:false},
  transferPage:{canDismiss:true,canClear:true},notifications:{available:false,popups:[],records:[]}};
let hints=vm.runInNewContext(guideExpression,footerContext);
assert.equal(hints.find(h=>h.key==='X').label,'Dismiss');
assert.equal(hints.find(h=>h.key==='X').show,true);
assert.equal(hints.find(h=>h.key==='Y').label,'Clear history');
assert.equal(hints.find(h=>h.key==='Y').show,true);
footerContext.transferPage.canDismiss=false;footerContext.transferPage.canClear=false;
hints=vm.runInNewContext(guideExpression,footerContext);
assert.equal(hints.find(h=>h.key==='X').show,false);
assert.equal(hints.find(h=>h.key==='Y').show,false);
const source = fs.readFileSync(path.join(__dirname,'../qml/TransferPage.qml'),'utf8');
const functions = source.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
const commands=[];
const job={id:'a'.repeat(32),batch:'batch',name:'Disc.iso',phase:'needs-destination',candidates:['ps1','ps2']};
const q={uiScale:1,available:true,busy:false,error:'',selectedId:'receiver',detailId:'',confirming:false,choice:0,
  snapshot:{jobs:[job],systems:[{id:'ps1',name:'PlayStation'},{id:'ps2',name:'PlayStation 2'}],enabled:false},
  list:{positionViewAtIndex:()=>{}},options:{positionViewAtIndex:()=>{}},ListView:{Contain:0},
  command:command=>commands.push(command)};
q.openScraping=(id,system,job)=>commands.push({action:'open-artwork',id,system,job});
q.page=q;
vm.createContext(q);
vm.runInContext(functions.join('\n'),q);
Object.defineProperties(q,{
  jobs:{get:()=>q.snapshot.jobs},
  historyCount:{get:()=>q.snapshot.historyCount || 0},
  selected:{get:()=>vm.runInContext(source.match(/readonly property var selected: ([^\n]+)/)[1],q)},
  blocked:{get:()=>vm.runInContext(source.match(/readonly property bool blocked: ([^\n]+)/)[1],q)},
  canDismiss:{get:()=>vm.runInContext(source.match(/readonly property bool canDismiss: ([^\n]+)/)[1],q)},
  canClear:{get:()=>vm.runInContext(source.match(/readonly property bool canClear: ([^\n]+)/)[1],q)},
  rows:{get:()=>vm.runInContext(source.match(/readonly property var rows: ([^\n]+)/)[1],q)},
  detail:{get:()=>vm.runInContext(source.match(/readonly property var detail: ([^\n]+)/)[1],q)},
  choices:{get:()=>vm.runInContext('(function() '+source.split('readonly property var choices: ')[1].split('\n    function px')[0].trim()+')()',q)}
});
q.activate();
assert.equal(commands.pop().action,'enable');
q.move(1);q.activate();
assert.equal(q.detailId,job.id);
q.move(1);q.activate();
assert.equal(commands.pop().system,'ps2');
assert.equal(q.detailId,'');
job.phase='needs-scrape';q.detailId=job.id;
assert.equal(q.choices.some(o=>['scrape','skip-scrape'].includes(o.action)),false,'Transfers do not ask for an artwork decision');
job.phase='complete';job.system='ps2';job.artwork_warning='No internet';q.choice=0;q.activate();
assert.equal(commands.pop().action,'open-artwork');assert.equal(q.detailId,'');
job.phase='needs-destination';job.artwork_warning='';
q.selectedId=job.id;q.activate();
q.choice=q.choices.findIndex(o=>o.action==='cancel');q.activate();
assert.equal(q.confirming,true);
assert.equal(commands.length,0,'Cancellation needs a deliberate second action');
q.choice=1;q.activate();
assert.equal(commands.pop().action,'cancel');
assert.equal(q.bytes(4294967313),'4.00 GiB');
const receiving={...job,phase:'receiving',size:4294967313,received:4294967313/4};
assert.equal(q.progress(receiving),.25);
assert.match(q.stage(receiving),/^Receiving · 25% · /);
receiving.received=receiving.size*.75;
assert.equal(q.progress(receiving),.75,'Progress follows the latest received byte count');
const failed={...job,phase:'failed',size:4284329308,received:4283957248};
assert.equal(q.percentage(failed),'99%','Rounded GiB totals must not imply a finished transfer');
assert.match(q.stage(failed),/^Failed · 99% · 3\.99 GiB \/ 3\.99 GiB$/);
assert.notEqual(q.progressColor(failed,false),q.progressColor({...failed,phase:'complete'},false));
assert.equal(q.progress({...receiving,size:0}),0);
assert.equal(q.progress({...receiving,received:Infinity}),0);
assert.equal(q.progress({...receiving,received:-1}),0);
assert.equal(q.progress({...receiving,received:receiving.size+1}),1);
for (const phase of ['validating','needs-scrape','scraping','ready','importing']) {
  const next={...receiving,phase,received:receiving.size};
  assert.equal(q.progress(next),0,'Post-transfer stages do not reuse the download percentage');
  assert.equal(q.stage(next).includes('100%'),false);
  assert.equal(q.working(next),phase!=='ready');
}
const artwork={...receiving,phase:'scraping',received:receiving.size};
assert.equal(q.stage(artwork),'Finding artwork & info…');
assert.equal(q.hint(artwork),'Your game has been received. You can keep playing while we finish.');
const ready={...artwork,phase:'ready'};
assert.equal(q.stage(ready),'Ready to add to your library');
assert.match(q.hint(ready),/Close this menu to update your library/);
ready.artwork_warning='Network unavailable';
assert.equal(q.stage(ready),'Ready to add without artwork');
assert.match(q.hint(ready),/You can find artwork later in Artwork & Game Info/);
assert.equal(q.stage({...ready,phase:'complete'}),'Added to your library');
assert.equal(q.hint({...ready,phase:'complete'}),'Your game is ready to play.');
for (const phase of ['offered','receiving','validating','ready','importing','needs-destination','needs-files']) {
  job.phase=phase;q.detailId='';q.selectedId=job.id;q.confirming=false;
  q.dismiss();
  assert.equal(commands.length,0,'X cannot dismiss pending work');
  assert.equal(q.canDismiss,false);
}
for (const phase of ['complete','failed','cancelled']) {
  job.phase=phase;q.detailId='';
  q.dismiss();
  const removed=commands.pop();
  assert.equal(removed.action,'dismiss-history');
  assert.equal(removed.id,job.id);
  assert.equal(q.detailId,'');
  q.detailId=job.id;q.dismiss();
  assert.equal(commands.length,0,'X is blocked in details, as in Notifications');
  assert.equal(q.choices.some(o=>o.action==='dismiss-history'),false,'No separate detail action duplicates the shortcut');
}
q.detailId='';
q.snapshot.historyCount=3;
q.clear();
assert.equal(q.detailId,'history');
assert.equal(commands.length,0,'Y opens a confirmation');
q.dismiss();q.clear();
assert.equal(commands.length,0,'Shortcuts do not bypass the confirmation');
assert.equal(q.back(),true);
assert.equal(commands.length,0,'Backing out keeps every record');
q.clear();q.activate();
assert.equal(q.detailId,'','Cancel is the default confirmation choice');
assert.equal(commands.length,0);
q.clear();q.choice=1;q.activate();
assert.equal(commands.pop().action,'clear-history');
assert.equal(q.detailId,'');
assert.equal(q.rows.some(r=>r.id==='history'),false,'The separate Clear history row is removed');
for (const [field,value] of [['busy',true],['available',false]]) {
  const previous=q[field];q[field]=value;
  q.dismiss();q.clear();
  assert.equal(commands.length,0);
  assert.equal(q.detailId,'');
  q[field]=previous;
}
q.snapshot.historyCount=0;q.clear();assert.equal(q.detailId,'');
const model=[];
q.inboxRows={get count(){return model.length;},get:i=>model[i],remove:i=>model.splice(i,1),
  insert:(i,row)=>model.splice(i,0,row),move:(i,j)=>model.splice(j,0,model.splice(i,1)[0])};
q.Qt={callLater:fn=>fn()};q.list.visible=true;
q.snapshot.jobs=['a','b','c'].map(id=>({...job,id}));q.selectedId='b';q.sync();
q.snapshot.jobs=q.snapshot.jobs.filter(row=>row.id!=='b');q.sync();
assert.equal(q.selectedId,'c','Dismissing a row keeps the nearest neighbor selected');
q.snapshot.jobs=q.snapshot.jobs.filter(row=>row.id!=='c');q.sync();
assert.equal(q.selectedId,'a','Dismissing the last row selects the previous neighbor');
q.snapshot.jobs.unshift({...job,id:'new'});q.sync();
assert.equal(q.selectedId,'a','New arrivals preserve selection');
q.snapshot.jobs=[];q.sync();
assert.equal(q.selectedId,'receiver','Clearing all records leaves the receiving action selected');
assert.equal(model.length,1);
job.phase='needs-destination';
const notificationSource=fs.readFileSync(path.join(__dirname,'../qml/NotificationsPage.qml'),'utf8');
const importsExpression=notificationSource.split('readonly property var liveImports: ')[1].split('\n    property real uiScale')[0].trim();
const live=vm.runInNewContext(importsExpression,{imports:[{...job,created:1}]});
const recordsExpression=notificationSource.match(/readonly property var records: ([^\n]+)/)[1];
const visible=vm.runInNewContext(recordsExpression,{liveImports:live,liveScrapes:[],liveTransactions:[],client:{records:[]}});
assert.equal(visible.length,1,'Cleared notification history leaves the durable inbox action visible');
assert.equal(visible[0].target,'transfer-pending');
console.log('PASS: controller-only receiving, destination selection, cancellation confirmation, large-byte presentation and history-independent inbox navigation.');
