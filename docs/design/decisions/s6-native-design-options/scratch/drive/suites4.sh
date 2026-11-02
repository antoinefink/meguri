#!/bin/bash
cd /Users/antoine/.herdr/worktrees/meguri/design
mkdir -p /tmp/claude-501
sum() { python3 -I - "$1" <<'PY'
import json,sys,glob,collections,re,os
d=os.path.dirname(sys.argv[1])
try:
    s=json.load(open(d+'/screens.json'))
    print('  screens:',[(x['screen'],x['status'],x['unsuppressed'],x['touchTargets']) for x in s])
except Exception: pass
for at in glob.glob(d+'/*-attachments/manifest.json'):
    m=json.load(open(at)); names={}
    def walk(o):
        if isinstance(o,dict):
            if 'exportedFileName' in o: names[o['exportedFileName']]=o.get('suggestedHumanReadableName','')
            for v in o.values(): walk(v)
        elif isinstance(o,list):
            for v in o: walk(v)
    walk(m)
    for f,n in names.items():
        if not n.startswith('a11y-screen'): continue
        x=json.load(open(os.path.dirname(at)+'/'+f)); c=collections.Counter()
        for i in x.get('findings',[])+x.get('touchTargets',[]):
            t=i if isinstance(i,str) else ' '.join(str(v)[:40] for k,v in i.items() if k in('auditType','identifier','label'))
            c[t[:90]]+=1
        if c: print('  ',n.split('_')[0],c.most_common(8))
PY
}
r() { local name=$1; shift; "$@" > /tmp/claude-501/s.txt 2>&1; local R; R=$(tail -1 /tmp/claude-501/s.txt); echo "$name $(jq -c '{status,message,failures:[.failures[]?|{id,message:(.message[:240])}][:5]}' "$R" 2>/dev/null | cut -c1-900) $R"; sum "$R" 2>/dev/null; }
r a11y-iphone tools/mg a11y --platform iphone --suite LocalReviewAccessibilityTests --suite ShellAccessibilityTests
r a11y-ipad tools/mg a11y --platform ipad --suite LocalReviewAccessibilityTests
