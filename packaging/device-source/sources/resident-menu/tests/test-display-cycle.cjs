const fs=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const assert=require('node:assert/strict');
const source=fs.readFileSync(path.join(__dirname,'../qml/DisplayPage.qml'),'utf8');
const methods=source.match(/^    function \w+\([^\n]*\) \{[^\n]*\}$|^    function \w+\([^\n]*\) \{[\s\S]*?^    \}/gm);
const mode=refresh=>({width:1920,height:1080,refresh});
const s={busy:false,preview:null,active:true,selected:0,rows:[{key:'refresh'}],
 output:{modes:[mode(60000),mode(60000),mode(59940),mode(50000),mode(50000)]},mode:mode(60000),notice:''};
vm.createContext(s);vm.runInContext(methods.join('\n'),s);
Object.defineProperties(s,{
 modes:{get:()=>s.distinctModes(s.output.modes)},
 rates:{get:()=>s.modes.filter(m=>m.width===s.mode.width && m.height===s.mode.height)},
 adjustable:{get:()=>true}
});
let checks=0;
assert.deepEqual(Array.from(s.modes,m=>m.refresh),[60000,59940,50000]);checks++;
for(const rate of [59940,50000,60000,59940,50000,60000]){
 s.horizontal(1);assert.equal(s.mode.refresh,rate);checks++;
}
for(const rate of [50000,59940,60000,50000,59940,60000]){
 s.horizontal(-1);assert.equal(s.mode.refresh,rate);checks++;
}
s.output.modes=[mode(60000),mode(60000)];
for(const direction of [-1,1]){s.horizontal(direction);assert.equal(s.mode.refresh,60000);checks++;}
s.output.modes=[mode(60000),mode(50000)];s.mode=mode(55000);
s.horizontal(1);assert.equal(s.mode.refresh,60000);checks++;
s.mode=mode(55000);s.horizontal(-1);assert.equal(s.mode.refresh,50000);checks++;
s.output.modes=[mode(60000),{width:1280,height:720,refresh:120000},mode(50000)];s.mode=mode(60000);
s.horizontal(1);assert.equal(s.mode.refresh,50000);assert.equal(s.mode.width,1920);checks++;
s.busy=true;s.horizontal(1);assert.equal(s.mode.refresh,50000);checks++;
s.busy=false;s.preview={token:'pending'};s.horizontal(1);assert.equal(s.mode.refresh,50000);checks++;
console.log(`PASS: ${checks} display cycling cases, including duplicate modes, fractional rates and both wrap directions.`);
