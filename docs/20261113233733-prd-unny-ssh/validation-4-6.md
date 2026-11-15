# Evidências das tasks 4, 5 e 6

Data: 14/11/2026. Ambiente: macOS/Darwin arm64, Apple M4, 16 GiB.
Código entregue; estado **Ready for QA** até validar a matriz macOS/Windows/Linux.

## Comportamento implementado

### Perfis e credenciais

- CRUD real dentro do payload criptografado do cofre. Publicação transacional:
  nonce novo em cada gravação, memória atualizada somente após publicação no disco.
  Validação de nome, host/IP, porta, usuário e diretório inicial absoluto.
- Lista e respostas de cadastro contêm metadados e `hasSavedSecret`, sem senha,
  chave privada, passphrase ou ID interno de segredo. Dados dinâmicos são inseridos
  por `textContent`/propriedades dos campos, sem interpolar conteúdo remoto em HTML.
- Políticas explícitas: manter credencial, salvar nova ou informar ao conectar sem
  salvar. Trocar modo de autenticação elimina a referência incompatível. Duplicar
  nunca copia a credencial. Excluir remove a referência e o segredo associado.
- Exclusão exige token de uso único ligado a perfil/geração, com validade de cinco
  minutos, além da confirmação contextual na interface. Cancelar não altera o cofre.
- Chaves são selecionadas por diálogo nativo no backend; a UI recebe somente nome
  do arquivo e um handle opaco. Não há acesso genérico ao filesystem da WebView.
  Limites: 1 MiB por chave, oito importações, cinco minutos de validade e limpeza
  periódica dos buffers expirados. Bloqueio limpa todas as importações.
- Senhas/passphrases são limpas dos campos antes do await do IPC. Segredos Rust
  usam zeroização; strings do payload são limpas ao descartar as cópias controladas.
  Isso não promete eliminar cópias internas da WebView ou de bibliotecas.

### SSH e identidade

- `russh =0.65.1` com ring/RSA e `russh-sftp =3.0.1`, fixados no Cargo.lock.
  Autenticação por senha, chave privada e Agent. Chaves OpenSSH Ed25519 e RSA SHA-2,
  incluindo chaves criptografadas, passaram nas verificações locais descritas abaixo.
- Agent: socket `SSH_AUTH_SOCK` no Unix e named pipe OpenSSH
  `\\.\pipe\openssh-ssh-agent` no Windows. Sem agent forwarding. São consideradas
  até cinco identidades do Agent para respeitar o orçamento de tentativas do servidor.
- Host desconhecido/alterado encerra o handshake sem enviar autenticação. Um desafio
  de cinco minutos mostra algoritmo, SHA-256, endereço e fingerprint anterior.
  Alteração conhecida exige também digitar o endereço. Rejeição/expiração não
  autentica. A confirmação persiste no cofre e a nova conexão verifica novamente a
  chave, evitando aceitar uma troca entre confirmação e reconexão.
- Comparação usa material criptográfico da chave, ignorando comentários OpenSSH.
  Reviews concorrentes verificam a identidade anteriormente salva antes de publicar;
  uma confirmação antiga não sobrescreve silenciosamente outra identidade confiada.
  Certificados de host são recusados como não suportados nesta versão.
- Timeout de 15 s para conexão/autenticação/abertura de PTY. Keepalive de 15 s com
  três falhas. Nenhuma repetição automática de autenticações, comandos ou gravações.
  VPN é apenas a rede externa pela qual o endereço fica alcançável.
- Uma tarefa possui cada conexão; fila de controle limitada a 16 mensagens. Sessões
  têm UUID, geração e snapshot do perfil. Edição/exclusão do cadastro não altera
  sessões abertas. Até 16 abas de sessão, com fechar, desconectar e conectar novamente.
- Capacidade SFTP é sondada por canal separado com limite de cinco segundos.
  Indisponibilidade não impede PTY. A árvore remota permanece para a task 7.
- Estados de sessão são consultados a cada segundo em uma lista limitada a 16 itens;
  intenção local de conectar aparece imediatamente. Terminal usa Channels, sem polling
  de saída. Essa decisão mantém notificações de ciclo de vida limitadas em memória.
- Autorização rápida por geração evita bloquear ACKs do terminal durante gravações
  de perfis. Bloqueio invalida acesso, cancela sessões/PTYs, limpa recursos e descarta
  a chave. Transporte TCP cancelável acorda leituras/escritas paradas. A integração
  com arquivos dirty, rascunhos e transferências continua reservada à task 10.

### Terminal

- PTYs reais, múltiplas abas e xterm/addon-fit. Cada instância mantém seu buffer ao
  trocar de aba/sessão. Contexto do servidor permanece no cabeçalho e na área terminal.
  Abertura automática do primeiro terminal após conectar, com novos shells por `+ Terminal`.
- Entrada, resize e Ctrl+C são enviados somente ao terminal de sua sessão/geração.
  Diretório inicial usa quoting POSIX e entrada no PTY após abrir o shell.
- Saída em bytes preserva UTF-8 dividido entre pacotes. Um bloco fica em trânsito
  por terminal; ACK ocorre após o callback de escrita do xterm. Consumidor parado
  por 30 s encerra o terminal. ACK inválido e entrada acima dos limites são rejeitados.
- Limites: oito terminais por sessão e 32 globais (incluindo abas encerradas na UI),
  scrollback de 5.000 linhas, pacote SSH de 32 KiB, janela de 256 KiB, oito mensagens
  por canal SSH e fila de entrada de 32 mensagens de até 64 KiB. Na UI, entrada é
  dividida em blocos de 16 KiB, com até 256 KiB e 64 chamadas pendentes. Resize é
  agregado por 80 ms. Não há filas/persistência de saída sem limite.
- Estados abrindo/ativo/ocioso/encerrado/erro são textuais. Fechar, desconectar,
  bloquear ou perder conexão interrompe os recursos; a UI descarta eventos tardios
  de outra sessão, geração, terminal ou sequência.
- OSC 52 e OSC 8 são consumidos sem ação; não há addon de links nem avaliação de
  saída como código. Copiar/colar usa ações locais explícitas e o plugin nativo
  de clipboard, com comandos restritos à janela principal e ao cofre aberto.
  Clipboard não é acessado por uma sequência enviada pelo servidor.
- Cmd+C com seleção (macOS) e Ctrl+Shift+C (Windows/Linux) copiam. A colagem usual
  passa pelo xterm e respeita bracketed paste quando habilitado pelo shell. Ctrl+C
  sem esses modificadores continua sendo interrupção. Ctrl+B/J dentro do terminal
  preservam o significado remoto; atalhos de layout continuam fora do terminal.

## Verificações executadas

| Verificação | Resultado local |
| --- | --- |
| `npm test` | 17 testes aprovados em nove arquivos |
| `npm run build` | Aprovado |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 24 aprovados; cinco helpers/fixtures ignorados por padrão |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Aprovado |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | Aprovado |
| `bash -n tests/fixtures/ssh/run-native.sh` | Aprovado |
| `bash tests/fixtures/ssh/run-native.sh` | Uma integração OpenSSH/Agent/PTy/SFTP aprovada separadamente |
| `npm run tauri build -- --debug --bundles app` | Bundle macOS gerado |
| `npm audit` | Zero vulnerabilidades reportadas |

Testes Rust usam servidor SSH real em loopback, com autenticação por senha/chave,
contagem de tentativas para provar que identidade é validada antes de autenticar,
revisão correta/incorreta de host alterado, rejeição, expiração, timeout, snapshot,
chave Ed25519 criptografada e passphrase incorreta. Dois terminais em duas sessões
verificam isolamento, UTF-8 em chunks, resize, interrupção, finalização, cancelamento
e 1 MiB de saída; sem ACK não é entregue outro bloco. Testes do cofre cobrem CRUD,
políticas de segredos, duplicação, acesso bloqueado e rollback por erro de escrita.

Vitest cobre os três formulários de autenticação e os payloads enviados, escolha
explícita de salvar, limpeza dos campos antes do IPC, cancelamento de exclusão,
seleção sem destruir o alvo do duplo clique e isolamento/cancelamento dos eventos.

A fixture nativa gera host key, Ed25519 e RSA criptografada temporários; inicia um
OpenSSH em loopback e um Agent próprio, sem reutilizar Agent ou configuração do
usuário. Autoriza somente as chaves descartáveis no processo de teste. Uma revisão
inicial de host é seguida de autenticação real Ed25519, RSA criptografada SHA-2 e
Agent, detecção SFTP positiva, stdin/stdout, `stty size` e Ctrl+C interrompendo um
`sleep`. A fixture remove diretórios e encerra seus processos ao terminar.

## Smoke pela interface macOS

Cofre isolado via `UNNY_SMOKE_DATA_DIR` em build debug; perfil e chave da fixture.
Não foi usado cofre pessoal. Observados no bundle `tauri://localhost`:

- Desbloquear → conectar por ação única usando a chave salva.
- Exibição de contexto real, estado conectado e disponibilidade SFTP independente
  do terminal (o primeiro smoke usou fixture sem SFTP; a integração seguinte validou SFTP).
- Execução de `printf` no shell remoto e saída visível no xterm.
- Segundo terminal, `sleep`, interrupção pelo botão Ctrl+C, colagem pelo sistema
  e nova saída. Voltar ao primeiro terminal preservou seu próprio buffer.
- Duplicação sem credencial, edição do nome, pesquisa e exclusão cancelada.
- Bloqueio com PTYs ativos; após abrir novamente, perfis preservados, sem sessões
  nem saída de terminal restauradas.
- O smoke identificou e levou à correção da comparação de chave com comentário.
  A regressão e a proteção de reviews concorrentes receberam testes Rust.

Os comandos nativos adicionais dos botões de clipboard foram compilados após esse
smoke; não se presume teste por cliques desses botões nem validação nos outros SOs.

## Pendências e limites

- Executar CI e smokes Windows/Linux, incluindo Agent named pipe, diálogo de arquivo,
  clipboard, fontes/atalhos e PTY. Sem runners conectados ou remoto Git neste workspace;
  workflow atualizado, sem alegar execução remota. Ambiente de VPN real não testado.
- A fixture Unix foi adicionada à CI macOS/Linux. No Windows permanecem os testes
  de servidor SSH em processo, além do roteiro de smoke manual a executar.
- Testar interrupção por perda física de rede, PTYs longos e clipboard em cada
  WebView. Timeout e cancelamento foram exercitados em loopback; isso não representa
  todos os modos de queda de uma VPN/rede real.
- Agent sem chaves/ausente tem erros tipados; essa indisponibilidade não foi exercitada
  na matriz nativa. Agents com mais de cinco identidades devem carregar a chave
  pretendida entre as primeiras, ou usar importação explícita.
- Sem suporte a certificados SSH, agent forwarding, jump hosts ou túneis. São
  funcionalidades fora do escopo atual. SFTP é apenas sondado; editor remoto,
  transferências e Docker ainda não executam operações reais.
- Monaco continua gerando aviso de chunk grande; apenas a prévia web o carrega nesta
  entrega. O aplicativo desktop mostra visão geral até a edição remota ser entregue.

Referências: [cliente russh](https://docs.rs/russh/0.65.1/russh/client/index.html),
[Agent russh](https://docs.rs/russh/0.65.1/russh/keys/agent/client/index.html) e
[segurança do xterm](https://xtermjs.org/docs/guides/security/).
