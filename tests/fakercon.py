import socket, struct, threading, sys
PW = b"secret123"
def pkt(i,t,b): d=struct.pack("<ii",i,t)+b+b"\0\0"; return struct.pack("<i",len(d))+d
def rd(c):
    h=b""
    while len(h)<4:
        x=c.recv(4-len(h)); 
        if not x: return None
        h+=x
    n=struct.unpack("<i",h)[0]; d=b""
    while len(d)<n:
        x=c.recv(n-len(d))
        if not x: return None
        d+=x
    i,t=struct.unpack("<ii",d[:8]); return i,t,d[8:-2]
def handle(c):
    while True:
        p=rd(c)
        if not p: break
        i,t,b=p
        if t==3: c.sendall(pkt(i if b==PW else -1,2,b""))
        elif t==2:
            cmd=b.decode()
            out = "There are 2 of a max of 8 players online: Alice, Bob" if cmd=="list" else f"ran:{cmd}"
            c.sendall(pkt(i,0,("\u00a7aOK " + out).encode()))
            if cmd=="stop": break
    c.close()
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(("127.0.0.1",int(sys.argv[1]))); s.listen(5)
while True:
    c,_=s.accept(); threading.Thread(target=handle,args=(c,),daemon=True).start()
