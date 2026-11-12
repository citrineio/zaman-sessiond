const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const root=path.join(__dirname,'..');
const read=name=>fs.readFileSync(path.join(root,name),'utf8');
const dukkan=read('qml/DukkanMark.qml');
const encoded=dukkan.match(/source: "data:image\/png;base64,([^"]+)"/)[1];
assert.deepEqual(Buffer.from(encoded,'base64'),fs.readFileSync(path.join(root,'assets/storefronts/dukkan-sign.png')),'runtime sign preserves exact supplied bytes');
assert.match(dukkan,/mipmap: true/);
assert.match(dukkan,/fillMode: Image.PreserveAspectFit/);
const state={};vm.createContext(state);vm.runInContext(read('qml/SteamPaths.js').replace('.pragma library',''),state);
function normalize(d){
 const t=d.match(/[MLCSZmlcsz]|[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:[eE][-+]?\d+)?/g),out=[];
 let i=0,command,x=0,y=0,sx=0,sy=0,previous='',control=null;
 while(i<t.length){
  if(/^[a-z]$/i.test(t[i]))command=t[i++];
  let kind=command.toUpperCase(),relative=command!==kind;
  if(kind==='Z'){out.push(['Z']);x=sx;y=sy;previous=kind;control=null;continue;}
  let n={M:2,L:2,C:6,S:4}[kind];assert.ok(n);let v=t.slice(i,i+n).map(Number);i+=n;
  if(relative)v=v.map((p,j)=>p+(j%2?y:x));
  if(kind==='S'){v=(previous==='C'?[2*x-control[0],2*y-control[1]]:[x,y]).concat(v);kind='C';}
  if(kind==='M'){[sx,sy]=v;command=relative?'l':'L';}
  control=kind==='C'?v.slice(2,4):null;previous=kind;[x,y]=v.slice(-2);
  out.push([kind,...v.map(p=>Math.round(p*1e6)/1e6)]);
 }
 return out;
}
assert.deepEqual(normalize('M0 0 c1 2 3 4 5 6 s7 8 9 10'),[['M',0,0],['C',1,2,3,4,5,6],['C',7,8,12,14,14,16]]);
const source=read('assets/storefronts/steam-light.svg');
const expected=[...source.matchAll(/<path d="([^"]+)"/g)].map(m=>normalize(m[1]));
assert.equal(expected.length,3);assert.deepEqual(JSON.parse(JSON.stringify(state.paths)),expected,'all official SVG curves are retained');
const calls=[];state.draw({save(){},restore(){},translate:(...p)=>calls.push(['translate',...p]),scale:(...p)=>calls.push(['scale',...p]),
 beginPath:()=>calls.push(['begin']),moveTo:(...p)=>calls.push(['M',...p]),lineTo:(...p)=>calls.push(['L',...p]),
 bezierCurveTo:(...p)=>calls.push(['C',...p]),closePath:()=>calls.push(['Z']),fill:()=>calls.push(['fill'])});
assert.equal(calls.filter(c=>c[0]==='fill').length,3);
assert.deepEqual(calls.filter(c=>['M','L','C','Z'].includes(c[0])),expected.flat());
const icon=read('qml/MenuIcon.qml');assert.doesNotMatch(icon,/data:image\/png/);assert.match(icon,/active: icon.branded/);
const shell=read('qml/shell.qml');assert.match(shell,/menuRow.storefrontMark \? 134 : 72/);assert.match(shell,/menuRow.dukkanMark \? 98/);
console.log('PASS: exact supplied sign, aspect/filtering, official Steam vector geometry, lazy loading and aligned storefront labels.');
