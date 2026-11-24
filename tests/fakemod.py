import http.server, json, hashlib, sys, urllib.parse
PORT=int(sys.argv[1]); JAR=b"PK-fake-plugin-bytes"
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def send(self,code,body,ct="application/json"):
        self.send_response(code); self.send_header("Content-Type",ct); self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        u=urllib.parse.urlparse(self.path); q=urllib.parse.parse_qs(u.query)
        if u.path=="/search":
            facets=q["facets"][0]; assert "categories:paper" in facets and "versions:1.21.10" in facets, facets
            self.send(200,json.dumps({"hits":[{"title":"Chunky","project_id":"abc","description":"Pre-generate chunks quickly and safely","downloads":1234,"author":"pop4959"}]}).encode())
        elif u.path=="/project/abc/version":
            assert q["loaders"][0]=='["paper"]' and q["game_versions"][0]=='["1.21.10"]', q
            self.send(200,json.dumps([{"version_number":"1.4.0","files":[{"primary":True,"filename":"Chunky-1.4.0.jar","url":f"http://127.0.0.1:{PORT}/cdn/Chunky-1.4.0.jar","hashes":{"sha512":hashlib.sha512(JAR).hexdigest()}}],"dependencies":[{"dependency_type":"required","project_id":"x"}]}]).encode())
        elif u.path=="/cdn/Chunky-1.4.0.jar": self.send(200,JAR,"application/java-archive")
        else: self.send(404,b"{}")
http.server.HTTPServer(("127.0.0.1",PORT),H).serve_forever()
