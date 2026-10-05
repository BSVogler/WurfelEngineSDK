# Deploying to Kubernetes

1. Build and push (from the repo root; add `--platform linux/amd64` on Apple Silicon):
   `docker build -t REGISTRY/wurfel-server:TAG . && docker push REGISTRY/wurfel-server:TAG`
2. Edit `kustomization.yaml` (image) and `ingress.yaml` (host, class, TLS).
3. Check with `kubectl kustomize deploy/k8s`, then `kubectl apply -k deploy/k8s` (mind your current context).

Notes: single replica with a `Recreate` strategy and a PVC, because the server owns one in-memory world
and saves it to `/data/maps` on SIGTERM. The client connects to `/ws` on its own origin, so the ingress must pass WebSockets.
