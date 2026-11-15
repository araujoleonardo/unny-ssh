# Ambientes descartáveis

Fixtures do cofre são geradas por `tempfile` durante `cargo test`; nenhuma senha,
chave privada ou envelope de usuário é versionado. A suíte usa senhas fictícias
somente em memória e exclui os diretórios temporários ao terminar.

OpenSSH/SFTP (iniciar somente em ambiente de testes):

```sh
cd tests/fixtures/openssh
mkdir -p runtime
ssh-keygen -t ed25519 -N '' -f runtime/id_ed25519
docker compose up --build -d
ssh -p 2222 -i runtime/id_ed25519 fixture@127.0.0.1
# Regenerar o container troca a identidade do host para o futuro teste da task 5.
docker compose down --volumes
```

A chave é descartável, local e ignorada pelo versionamento. O servidor só publica
uma porta no loopback, não admite senha, root, túneis ou agent forwarding.
A fixture prepara autenticação/SFTP; não comprova integração SSH antes da task 5.

Para Docker, use uma VM dedicada. `docker-vm/Vagrantfile` prepara Ubuntu 22.04 e
um daemon interno, sem montar o socket Docker ou pastas do host. Requer Vagrant,
VirtualBox e uma box compatível com a arquitetura do host. Em Apple Silicon,
crie uma VM ARM equivalente com seu hipervisor e instale Docker dentro dela.

```sh
cd tests/fixtures/docker-vm
vagrant up
vagrant ssh
# executar os cenários Docker exclusivamente nessa VM
vagrant destroy -f
```

A receita Vagrant não foi executada. Para as tasks 10–12 foi usada uma VM
OrbStack ARM64 dedicada: consulte [Docker/Compose isolados](docker/README.md).
Não utilizar o daemon de produção como substituto.

Para integração OpenSSH/Agent/PTy nativa, sem Docker, consulte [ssh/README.md](ssh/README.md). Essa fixture foi executada no macOS para as tasks 4–6.
