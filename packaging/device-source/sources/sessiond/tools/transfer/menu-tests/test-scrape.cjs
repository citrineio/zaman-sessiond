const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {fixture}=require('./menu-controller-regression.cjs');
const shell=fixture('transfer');shell.returnPage='';
shell.openScraping('','ps2','');assert.equal(shell.page,'scrape');
assert.equal(shell.pageEntry.label,'Artwork & Game Info');
shell.back();assert.equal(shell.page,'transfer');assert.equal(shell.returnPage,'');
shell.back();assert.equal(shell.page,'');
const route=fixture('scrape');route.notifications.available=false;
route.input('ui_context',1);route.input('ui_context',1);route.input('ui_context',0);
route.input('ui_action',1);route.input('ui_action',0);
assert.equal(route.actions.join(','),'scrape-dismiss,scrape-clear');
const source=fs.readFileSync(path.join(__dirname,'../qml/ScrapePage.qml'),'utf8');
const functions=source.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
const commands=[],keys={secret:'',reset(){this.secret='';},navigate(){},cycleView(){},activate(){},append(s){this.secret+=s;},erase(){this.secret=this.secret.slice(0,-1);}};
const q={uiScale:1,snapshot:{available:true,folders:[{id:'ps2',name:'PlayStation 2',supported:true},{id:'others',name:'Other',supported:false,reason:'Mixed systems'}],jobs:[],historyCount:0},
 available:true,busy:false,error:'',view:'setup',folderId:'all',importId:'',importSystem:'',jobId:'',mode:0,review:false,selected:1,knownJobs:[],pendingStart:false,
 command:r=>commands.push(r),closeRequested:()=>commands.push({action:'close-menu'}),list:{positionViewAtIndex(){}},ListView:{Contain:0},answerKeys:keys,logView:{contentY:0,contentHeight:1000,height:100}};
q.page=q;vm.createContext(q);vm.runInContext(functions.join('\n'),q);
for(const name of ['jobs','activeJobs','job','prompt','promptKey','jobPhase','editingText','canDismiss','canClear','modes']) {
 const expr=source.match(new RegExp('readonly property (?:var|bool|string) '+name+': ([^\\n]+)'))[1];
 Object.defineProperty(q,name,{get:()=>vm.runInContext(expr,q)});
}
Object.defineProperty(q,'rows',{get:()=>vm.runInContext('(function() '+source.split('readonly property var rows: ')[1].split('\n    function px')[0].trim()+')()',q)});
assert.equal(q.rows.length,4,'Default page has only collection, primary action, Options and activity');
assert.equal(q.rows[1].kind,'primary');assert.equal(q.rows[1].label,'Find artwork & info');
for (let index=0;index<=q.rows.length;index++) {q.selected=index;assert.equal(q.canDismiss,false);}
q.selected=0;q.activate();assert.equal(q.view,'folders');assert.equal(q.rows[0].folder,'all');
q.selected=2;q.activate();assert.equal(q.view,'folders','Unsupported collections cannot start a job');
q.selected=1;q.activate();assert.equal(q.folderId,'ps2');assert.equal(q.selected,1);
q.activate();let request=commands.pop();assert.equal(request.action,'scrape-start');assert.equal(request.mode,'cached');assert.equal(request.review,false);
q.pendingStart=false;q.selected=2;q.activate();assert.equal(q.view,'options');
q.selected=1;q.activate();assert.equal(q.mode,1);q.selected=3;q.activate();assert.equal(q.review,true);
q.selected=4;q.activate();assert.equal(q.view,'setup');q.activate();assert.equal(q.view,'confirm');assert.equal(q.selected,1);
q.activate();assert.equal(q.view,'setup');assert.equal(commands.length,0,'Replacing existing info requires confirmation');
q.activate();q.selected=0;q.activate();request=commands.pop();assert.equal(request.mode,'refresh');assert.equal(request.review,true);
const job={id:'a'.repeat(32),created:1,phase:'needs-input',folders:['ps2'],imports:[],stage:'Your choice',prompt:{id:'p1',kind:'match',question:'Which game?',choices:[{label:'No',value:'n'},{label:'Yes',value:'y'}]},log:'test',results:[]};
q.snapshot.jobs=[job];vm.runInContext(source.match(/    onJobsChanged: \{([\s\S]*?)^    \}/m)[1],q);
assert.equal(q.view,'job');assert.equal(q.jobId,job.id);
const promptChange=source.match(/    onPromptKeyChanged: (\{[^\n]+\})/)[1];
assert.ok(!source.includes('onPromptChanged:'));
let previousPromptKey=q.promptKey;q.selected=1;
for(let i=0;i<4;i++) {q.snapshot=JSON.parse(JSON.stringify(q.snapshot));q.snapshot.jobs[0].log='Poll '+i;if(q.promptKey!==previousPromptKey)vm.runInContext(promptChange,q);previousPromptKey=q.promptKey;assert.equal(q.selected,1);}
q.snapshot.jobs[0].prompt.id='next';vm.runInContext(promptChange,q);assert.equal(q.selected,0);
q.snapshot.jobs=[job];q.selected=1;q.activate();request=commands.pop();assert.equal(request.answer,'y');assert.equal(request.prompt,'p1');
job.prompt={id:'p2',question:'Text?',text:true,choices:[]};q.selected=0;q.activate();assert.equal(q.editingText,true);keys.secret='answer';q.submitAnswer(keys.secret);assert.equal(commands.pop().answer,'answer');assert.equal(keys.secret,'');
job.prompt=null;job.phase='running';q.view='job';q.selected=0;q.activate();assert.equal(commands.pop().action,'close-menu');
q.open('','ps2','');assert.equal(q.rows[1].action,'active-job');q.activate();assert.equal(q.view,'job');
job.phase='complete';q.snapshot.historyCount=1;q.view='setup';q.selected=3;q.activate();assert.equal(q.view,'activity');assert.equal(q.canDismiss,true);
q.dismiss();assert.equal(commands.pop().action,'scrape-dismiss');
q.clear();q.activate();assert.equal(q.view,'activity');assert.equal(commands.length,0);
q.clear();q.selected=1;q.activate();assert.equal(commands.pop().action,'scrape-clear');
q.open('','ps2','');q.mode=2;q.review=true;q.activate();request=commands.pop();assert.equal(request.mode,'offline');assert.equal(request.review,false);
const notices=fs.readFileSync(path.join(__dirname,'../qml/NotificationsPage.qml'),'utf8');
const activate=notices.match(/    function activate\(\) \{([\s\S]*?)^    \}/m)[1];
const calls=[];const context={confirming:false,detail:null,selected:{target:'transfer-pending'},openTransfer:()=>calls.push('transfer')};
vm.runInNewContext('(function(){'+activate+'})()',context);assert.equal(calls[0],'transfer');
const visibleRows=[];
q.activityRows={get count(){return visibleRows.length;},get:i=>visibleRows[i],remove:i=>visibleRows.splice(i,1),
 insert:(i,row)=>visibleRows.splice(i,0,row),move:(i,j)=>visibleRows.splice(j,0,visibleRows.splice(i,1)[0])};
q.Qt={callLater:fn=>fn()};q.list.visible=true;q.syncedIds=[];q.syncedView='';
q.pendingStart=false;q.snapshot.jobs=['a','b','c'].map(id=>({...job,id,prompt:null,phase:'complete'}));q.view='activity';q.selected=1;q.syncRows();
assert.equal(q.animateActivityRemovals,false,'Entering activity does not animate setup controls out');
const b=visibleRows[1];q.snapshot.jobs=JSON.parse(JSON.stringify(q.snapshot.jobs));q.syncRows();
assert.equal(visibleRows[1],b,'Status polls keep delegates instead of rebuilding the list');
q.snapshot.jobs.unshift({...job,id:'new',prompt:null,phase:'complete'});q.syncRows();
assert.equal(q.rows[q.selected].id,'b','New arrivals preserve the selected artwork job');
q.snapshot.jobs=q.snapshot.jobs.filter(j=>j.id!=='b');q.syncRows();
assert.equal(q.rows[q.selected].id,'c','Dismissing artwork selects its nearest neighbor');
assert.equal(q.animateActivityRemovals,true);
q.jobId='c';q.snapshot.jobs=q.snapshot.jobs.filter(j=>j.id!=='c');q.syncRows();
vm.runInContext(source.match(/    onJobsChanged: \{([\s\S]*?)^    \}/m)[1],q);
assert.equal(q.view,'activity','Removing a previously opened job does not leave the history view');
q.snapshot.jobs=[];q.syncRows();
assert.equal(q.rows[q.selected].id,'back','Clearing history leaves the back action selected');
q.view='setup';q.syncRows();assert.equal(q.animateActivityRemovals,false,'Navigation is not a history dismissal');
console.log('PASS: recommended primary action, four-row setup, optional configuration, replacement confirmation, stable question focus, automatic-transfer navigation and X/Y history.');
