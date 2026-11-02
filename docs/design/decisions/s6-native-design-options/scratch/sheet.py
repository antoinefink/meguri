import sys
from PIL import Image
S=sys.argv[1]; ims=[Image.open(S+n+".png").convert("RGB") for n in sys.argv[2:]]
h=900; rs=[i.resize((int(i.width*h/i.height),h)) for i in ims]
c=Image.new("RGB",(sum(r.width for r in rs)+30*(len(rs)-1),h),(120,120,120)); x=0
for r in rs: c.paste(r,(x,0)); x+=r.width+30
c.save(S+"sheet.png"); print(c.size,[r.width for r in rs])
