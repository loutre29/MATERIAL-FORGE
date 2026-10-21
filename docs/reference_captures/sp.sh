# usage: sp.sh js|python 'code'
b=$(printf '%s' "$2" | base64 -w0)
curl -s -m 120 -X POST -H "Content-Type: application/json" -d "{\"$1\":\"$b\"}" http://localhost:60041/run.json; echo
