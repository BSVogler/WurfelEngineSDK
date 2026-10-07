# CI/CD to k3s

`.github/workflows/deploy.yml` runs on every push to `master` / `rust-port-wip`: tests, builds the image,
pushes `ghcr.io/bsvogler/wurfel-server:<branch>-<sha>`, then `kubectl apply -k deploy/k3s` and waits for the rollout.

## One-time setup

1. Hostname: the test deployment is served at `https://wurfel-test.portraittogo.com` (`httproute.yaml`). The
   cluster's Gateway (`kube-system/traefik-gateway`) already has a `*.portraittogo.com` listener with a wildcard
   certificate, and wildcard DNS points at it, so nothing else is needed for that name. For a name on another
   domain you need a DNS record to the Gateway IP, plus a listener and certificate for it on the Gateway.
2. As cluster admin: `kubectl apply -f deploy/k3s/bootstrap/deployer-rbac.yaml`
3. Build a kubeconfig for the `ci-deployer` account and store it base64-encoded as the GitHub secret
   `K3S_KUBECONFIG` (repo Settings > Secrets, ideally on the `production` environment):
   ```
   NS=wurfel
   SERVER=$(kubectl config view --minify -o jsonpath='{.clusters[0].cluster.server}')
   CA=$(kubectl -n $NS get secret ci-deployer-token -o jsonpath='{.data.ca\.crt}')
   TOKEN=$(kubectl -n $NS get secret ci-deployer-token -o jsonpath='{.data.token}' | base64 -d)
   cat <<EOF | base64 | tr -d '\n'
   apiVersion: v1
   kind: Config
   clusters: [{name: k3s, cluster: {server: $SERVER, certificate-authority-data: $CA}}]
   users: [{name: ci, user: {token: $TOKEN}}]
   contexts: [{name: ci, context: {cluster: k3s, user: ci, namespace: $NS}}]
   current-context: ci
   EOF
   ```
   The k3s API (port 6443) must be reachable from GitHub's runners.
4. After the first push, make the ghcr package `wurfel-server` public (Packages > Package settings), or the
   cluster cannot pull it without an `imagePullSecret`.
