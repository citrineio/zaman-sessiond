const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync(path.join(__dirname,'../theme.qml'),'utf8');
const inspect=source.match(/^    function inspectRomRefreshSignal\(\) \{[\s\S]*?^    \}/m)[0];
const trigger=source.split('id: romRefreshDebounce')[1].match(/onTriggered: \{([\s\S]*?)\n        \}/)[1];
const memory=new Map([['native','keep-native'],['steam','keep-steam']]);
let reloads=0,starts=0,retries=0;
const s={FolderListModel:{Ready:1},romRefreshMemoryKey:'rom',
  api:{memory:{has:k=>memory.has(k),get:k=>memory.get(k),set:(k,v)=>memory.set(k,v)}},
  romRefreshSignals:{status:1,count:1,get:()=> 'rom-one.signal'},
  romRefreshDebounce:{running:false,start(){this.running=true;starts++;}},
  Window:{window:{active:false}},mainShell:{launching:false},
  Internal:{settings:{reloadProviders:()=>reloads++}},restart:()=>retries++};
s.root=s;
vm.createContext(s);vm.runInContext(inspect,s);
s.inspectRomRefreshSignal();s.inspectRomRefreshSignal();
assert.equal(starts,1,'Polling must not continuously restart the pending timer');
vm.runInContext('(function(){'+trigger+'})()',s);assert.equal(reloads,0);assert.equal(memory.has('rom'),false);assert.equal(retries,1);
s.Window.window.active=true;s.mainShell.launching=true;vm.runInContext('(function(){'+trigger+'})()',s);assert.equal(reloads,0);
s.mainShell.launching=false;vm.runInContext('(function(){'+trigger+'})()',s);assert.equal(reloads,1);assert.equal(memory.get('rom'),'rom-one.signal');
s.romRefreshDebounce.running=false;s.inspectRomRefreshSignal();assert.equal(starts,1);
assert.equal(memory.get('native'),'keep-native');assert.equal(memory.get('steam'),'keep-steam');
s.romRefreshSignals.count=2;s.inspectRomRefreshSignal();assert.equal(starts,1);
console.log('PASS: ROM refresh defers through lost focus/launching, acknowledges only a reload, avoids polling starvation and preserves other namespaces.');
