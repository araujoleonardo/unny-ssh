# Docker/Compose em VM descartável

A fixture cria uma VM **nova e isolada** pelo OrbStack, sem compartilhamento de
pastas, encaminhamento de Agent ou montagem do socket Docker do Mac. Instala
Docker Engine, Compose v2 e OpenSSH **dentro da VM**. O Docker do host nunca é
consultado nem usado para executar os cenários.

Requisitos no Mac: OrbStack em execução, Python 3, Bash, ferramentas OpenSSH,
Rust e Node/npm. Execute a partir da raiz do repositório:

```sh
bash tests/fixtures/docker/run-orb.sh
```

O script gera chave e cofre sintéticos em diretório temporário, usa usuários SSH
próprios e uma ponte TCP no loopback para o `sshd` da VM isolada. O `sshd` da
fixture também escuta apenas no loopback interno. Ao terminar, remove somente a
máquina cujo nome único ele criou e seus arquivos temporários. Não reutiliza VM,
Agent, configuração SSH, cofre nem daemon de produção. O provisionamento baixa
pacotes Ubuntu e a imagem oficial `busybox:1.37.0` dentro da VM.

Cenários: inventário/inspect; start/stop/restart/pause/unpause/remove; dependências
em uso; confirmação exata de volume; identidade e revisão de tokens; logs com ACK
e cancelamento; exec PTY; Compose em caminho com espaço/apóstrofo; up/start/stop/
restart/down; dados preservados no down padrão e remoção adicional autorizada;
usuário sem permissão; Docker ausente do PATH; Compose ausente; bloqueio com PTY,
logs, consulta e transferência pendentes, rascunho e recuperação sem salvar no
servidor. O script verifica processos de acompanhamento após encerramento ordenado.

Para smoke do WebView macOS, gere o bundle debug e use:

```sh
npm run tauri build -- --debug --bundles app
bash tests/fixtures/docker/run-orb.sh --desktop
```

Não abra outra instância do aplicativo durante esse smoke. A senha sintética
está no helper `native_docker_seed`; o caminho de cofre de teste só é aceito em
build debug. Ao fechar a aplicação, o script elimina sua VM e dados temporários.
`setup-vm.sh` deve ser executado exclusivamente dentro da VM dedicada e exige
`UNNY_DISPOSABLE_DOCKER=1`. Não o execute em um servidor pessoal/de produção.

A execução local usou macOS/Apple Silicon como cliente e Ubuntu 24.04 ARM64 como
servidor, Docker 29.1.3 e Compose 2.40.3. Isso comprova integração com servidor
Linux, não o aceite da WebView do cliente Linux ou Windows. A matriz desktop e
cenários físicos de falha permanecem na documentação da feature.
