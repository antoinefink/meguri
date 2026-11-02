import json,sys,glob,collections,os
for r in sys.argv[1:]:
    d=os.path.dirname(r); j=json.load(open(r)); print('##',j['commit']['sha'][:8],' '.join(j['args'])[:60])
    for at in glob.glob(d+'/*-attachments/manifest.json'):
        m=json.load(open(at)); names={}
        def walk(o):
            if isinstance(o,dict):
                if 'exportedFileName' in o: names[o['exportedFileName']]=o.get('suggestedHumanReadableName','')
                for v in o.values(): walk(v)
            elif isinstance(o,list):
                for v in o: walk(v)
        walk(m)
        for f,n in sorted(names.items(),key=lambda x:x[1]):
            if not n.startswith('a11y-screen'): continue
            x=json.load(open(os.path.dirname(at)+'/'+f)); c=collections.Counter()
            for i in x.get('findings',[])+x.get('touchTargets',[]):
                t=i if isinstance(i,str) else ' '.join(str(v)[:30] for k,v in i.items() if k in('auditType','identifier','label'))
                c[t.split(': ',1)[-1][:70]]+=1
            print(' ',n.split('_')[0].replace('a11y-screen.',''),c.most_common(9))
