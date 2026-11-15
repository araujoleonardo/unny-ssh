# unny-ssh

Aplicativo desktop com Tauri 2, Rust e TypeScript/Vite para reunir conexões SSH,
arquivos, terminal e Docker em um ambiente de trabalho.

A entrega atual inclui cofre criptografado, cadastro de conexões, sessões SSH reais
por senha/chave/Agent, terminais PTY com abas, explorador SFTP, editor remoto e
transferências com ZIP, rascunhos recuperáveis, Docker e projetos Compose. A prévia web mantém ambientes
simulados identificados; conexões reais funcionam no aplicativo desktop.

## Desenvolvimento

Requisitos: Node.js 24, npm, Rust e as dependências de sistema do
[Tauri](https://v2.tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev
```

Se o Rust não estiver no PATH:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

`npm run dev` abre somente a prévia web. O cofre real está disponível no aplicativo
desktop; a prévia não simula armazenamento de senhas.

## Cofre e layout

No primeiro uso, crie e confirme uma senha mestra com pelo menos 12 caracteres.
Não existe recuperação de senha. O botão **Cofre desbloqueado** permite trocar a
senha; **Bloquear** protege rascunhos automaticamente e encerra sessões,
terminais, transferências e acompanhamentos Docker. O seletor **Bloquear após**
configura a inatividade entre 1 e 240 minutos (padrão 15). Só interação local
reinicia o relógio; saída remota não o mantém aberto. A ocultação da janela e a
retomada após uma pausa do relógio também protegem o contexto; macOS recebe
notificações nativas de suspensão/desligamento.

Documentos dirty são capturados em blobs individuais criptografados em
`vault/drafts/`, com nomes opacos. O índice é publicado somente após os blobs.
Trocar a senha recriptografa os blobs antes de publicar a nova geração.
Após desbloquear/reiniciar, **Rascunhos** permite visualizar o conteúdo e recuperar
em uma sessão conectada ao perfil/endereço originais. Recuperar não salva no
servidor; use Salvar explicitamente, com comparação da revisão original. O
rascunho protegido permanece até você excluí-lo ou protegê-lo novamente.

Falha de disco exibe **Bloqueio incompleto / lock_failed**: conteúdo oculto,
sessões encerradas, buffers e chave retidos em memória. Autentique para tentar
proteger novamente ou descartar explicitamente os pendentes. Não force a saída
nessa condição. Falhas anteriores à captura mantêm o workspace disponível com
mensagem de bloqueio não concluído. Fechar a aplicação oferece salvar, descartar,
proteger rascunhos ou cancelar quando há alterações. Nada é repetido remotamente
ao reabrir.

A opção de redefinição na tela bloqueada exige digitar `APAGAR MEU COFRE` e criar
uma nova senha. Ela substitui os dados locais sem recuperação. Senha incorreta,
formato inválido ou adulteração não recriam o arquivo automaticamente.

O backend guarda o envelope em `appLocalDataDir/vault/vault.json`, com Argon2id
(64 MiB, três iterações, paralelismo 1) e XChaCha20-Poly1305. Chaves ficam no backend; texto em edição também ocupa a memória da WebView. Unix usa diretório 0700 e arquivos 0600; Windows usa ACL
restrita ao usuário via PowerShell, ainda pendente de validação nativa. Logs locais
contêm apenas códigos de operação/resultado e duração, com rotação de até 10 MiB.

Arraste os divisores ou use suas setas de teclado para redimensionar os painéis.
`Home` e `End` levam aos limites. Os botões laterais recolhem/restauram conexões e
terminal; **Restaurar layout** recupera as dimensões iniciais. Só preferências de
layout e tempo de inatividade são gravadas fora do cofre.

- macOS: `Cmd+B` alterna a lateral, `Cmd+J` alterna o terminal e `Cmd+Shift+L` bloqueia.
- Windows/Linux: `Ctrl+B`, `Ctrl+J` e `Ctrl+Shift+L`, respectivamente.

## Conexões SSH e terminal

Use **+** em Conexões para cadastrar nome, host/IP, porta, usuário, autenticação e
pasta inicial absoluta opcional. Escolha explicitamente salvar uma credencial no
cofre ou informá-la apenas ao conectar. Chaves são importadas pelo seletor nativo.
**Duplicar** cria uma cópia sem credencial; **Excluir** pede confirmação contextual.

Clique no botão de conectar ou dê duplo clique no perfil. No primeiro acesso,
compare a impressão digital do servidor com uma fonte confiável antes de aceitar.
Uma chave conhecida alterada exige revisão e confirmação com o endereço do servidor.
A checagem ocorre antes de enviar credenciais; rejeição e expiração não autenticam.

Senha, OpenSSH Ed25519/RSA SHA-2 (inclusive chaves criptografadas) e SSH Agent são
suportados. No Unix, o Agent usa `SSH_AUTH_SOCK`; Windows usa o named pipe do
OpenSSH Agent. Não há agent forwarding. Confira a rede/VPN externamente: o aplicativo
não configura VPN. Falhas exigem nova tentativa manual, sem repetir comandos.

Cada sessão mantém o snapshot do perfil e seus próprios terminais. **+ Terminal**
abre outro shell; **Ctrl+C** interrompe o processo ativo. Copiar usa seleção e
Cmd+C no macOS ou Ctrl+Shift+C em Windows/Linux; colar usa o atalho do sistema ou
os botões locais. Ctrl+B/J dentro do terminal preservam o significado remoto.
Saída, links e sequências remotas não podem manipular o clipboard automaticamente.

Até 16 sessões, oito terminais por sessão e 32 abas de terminal no total, incluindo
abas encerradas. Feche abas antigas para liberar capacidade. Scrollback é limitado
a 5.000 linhas, sem gravação da saída no disco. Bloquear encerra sessões e descarta
buffers do terminal; documentos dirty são protegidos como rascunhos.

## Validação e build

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
npm run tauri build
```

A CI em `.github/workflows/native.yml` prepara builds e testes para macOS,
Windows e Ubuntu. Isso não substitui o smoke da WebView de cada plataforma.
Consulte [as evidências e limitações](docs/20261113233733-prd-unny-ssh/validation-1-3.md)
das tasks 1–3, [as evidências das tasks 4–6](docs/20261113233733-prd-unny-ssh/validation-4-6.md)
e [o estado das tasks](docs/20261113233733-prd-unny-ssh/state.md).

O frontend fica em `src/app`, `src/features` e `src/ipc`; os comandos, serviços e
adaptadores ficam em `src-tauri/src`. Os ambientes descartáveis estão descritos em
[tests/fixtures](tests/fixtures/README.md).

## Arquivos remotos

Conecte uma sessão com SFTP e expanda pastas no **Explorador**. A árvore consulta
apenas pastas abertas, pagina listagens e mantém poucas linhas no DOM. **Pasta pai**
permite subir do diretório inicial; **Atualizar** relê a pasta atual. Selecione uma
entrada antes de renomear, excluir ou baixar. Pastas só são excluídas quando vazias;
links são identificados e sua exclusão não remove o destino. Handles internos
preservam nomes remotos que não são UTF-8; a interface escapa bytes inválidos.

Abra um arquivo com duplo clique ou Enter. O Monaco aceita texto UTF-8 de até
5 MiB; outros formatos oferecem download. Use **Salvar** ou Cmd/Ctrl+S; busca e
substituição usam os atalhos do Monaco. Abas pertencem à sessão e mantêm alterações
em memória durante falhas/desconexões. BOM, finais de linha e permissões aplicáveis
são preservados. Em arquivos com finais mistos, separadores seguem a posição das
linhas originais; linhas adicionais usam o separador predominante.

Antes de gravar, a revisão compara tamanho, mtime e SHA-256. Conflitos permitem
recarregar, cancelar ou sobrescrever explicitamente. A gravação usa temporário
irmão e rename POSIX quando disponível. Sem essa extensão, exige autorização para
uma gravação que pode ficar parcial numa queda. SFTP não oferece compare-and-swap:
ainda existe uma janela entre a última comparação e a substituição.

**Upload** abre o seletor nativo. Também é possível soltar arquivos na janela e
confirmar o diretório remoto. **Baixar / ZIP** abre um seletor de destino local;
pastas viram ZIP64 sem seguir links ou arquivos especiais, com resumo dos ignorados.
Downloads locais ficam fora da proteção do cofre. Colisões exigem sobrescrever,
renomear ou ignorar. Há duas transferências ativas por sessão (quatro no total),
com fila, progresso e cancelamento. Downloads incompletos permanecem `.part`;
uploads usam temporário remoto e informam resíduos quando a limpeza falha.

Teste descartável de SFTP/arquivos/transferências no macOS ou Linux:

```sh
bash tests/fixtures/ssh/run-native.sh --files
```

[Evidências e limitações das tasks 7–9](docs/20261113233733-prd-unny-ssh/validation-7-9.md).

## Docker e Compose

Abra **Docker** em uma sessão SSH conectada. Containers, imagens e volumes têm
pesquisa e atualização; inspeções detalhadas ficam apenas na interface. O servidor
precisa de Linux com Docker Engine no socket local padrão
`unix:///var/run/docker.sock` e permissão do usuário SSH. O aplicativo não instala
Docker, não executa sudo e não segue `DOCKER_HOST`/`DOCKER_CONTEXT` herdados para
outro daemon. Sockets rootless alternativos e contextos Docker remotos não são
configuráveis nesta entrega.

As ações de containers respeitam o estado observado e pedem confirmação com
servidor/recurso. Remover imagem ou volume em uso retorna erro, sem `--force`.
Remover volume exige digitar seu nome e confirma perda de dados. Remover container
não inclui volumes. **Logs** acompanha a saída em canal próprio; **Terminal
/bin/sh** abre um PTY no container (imagens sem `/bin/sh` informarão erro).

Em **Compose**, registre um diretório absoluto com configuração válida do Compose
v2. O registro pertence ao perfil e endereço originais e fica no cofre. Consulte
os serviços e use up/start/stop/restart/down acompanhando stdout/stderr e resultado.
Up usa modo detached; down padrão preserva volumes. Marcar a opção adicional de
volumes ainda exige uma segunda confirmação, com texto específico do projeto.
Confirmações expiram em dois minutos e deixam de valer se o estado/configuração
observável mudar. CLI Docker/Compose não oferece CAS transacional; há janela
residual entre a verificação e a ação.

Cancelar acompanhamento, desconectar ou bloquear pede encerramento dos canais
antes de fechar SSH, com espera limitada. Efeitos de comandos já aceitos pelo
servidor podem permanecer; confira o estado antes de tentar novamente. Não há
reconexão, replay nem rollback automáticos. Histórico de saída Docker é limitado
a 256 KiB por operação e não é persistido no disco.

[Fixture Docker/Compose isolada](tests/fixtures/docker/README.md) e
[evidências das tasks 10–12](docs/20261113233733-prd-unny-ssh/validation-10-12.md).

## Navegação do workspace

O seletor na barra superior reúne conexões salvas e sessões abertas. Sem sessões,
as conexões aparecem na lateral. Ao conectar, a lateral mostra a árvore remota;
use o botão direito em uma pasta para **Abrir no terminal**, criar, baixar,
renomear ou excluir entradas. A exclusão pede confirmação e pastas precisam estar
vazias. O terminal ocupa a faixa inferior e pode ser redimensionado ou recolhido.

[Layout, navegação e evidências de validação](docs/workspace-layout.md).

## Instalador macOS

A [preparação da release macOS](docs/macos-release.md) registra os comandos,
resultados e pendências de assinatura/notarização. Para preparar o `.app` local
com assinatura ad hoc: `npm run build:macos:app:local`. Para gerar o DMG local:
`npm run package:macos:local`. Esses comandos não produzem uma release notarizada.

## Instalador Windows

O workflow [Windows release](.github/workflows/release-windows.yml) gera um
instalador Windows x64 e publica na release ao enviar uma tag como `v0.1.0`.
Consulte [os passos e requisitos de versão](docs/windows-release.md).

## Importação e exportação de conexões

Na lateral ou no seletor de conexões, use **Importar JSON** / **Exportar JSON**.
O formato `ssh-client-backup` v1 preserva os dados das conexões, sem
senhas, passphrases ou chaves privadas. A importação pede confirmação, adiciona
novas conexões e ignora IDs existentes. [Formato e comportamento](docs/connections-json.md).
