# Evidências das tasks 10–12

Data: 14/11/2026. Cliente local: macOS 27.2.1 ARM64, Node 24.22.1,
Rust 1.99.1. Servidor: VM Ubuntu 24.04 ARM64 criada exclusivamente para estes
cenários, Docker Engine 29.1.3, Compose 2.40.3.
Estado: **Ready for QA**; não representa aceite dos três clientes desktop.

## Bloqueio e rascunhos

- Coordenador com participantes registráveis, captura dos modelos dirty e escudo
  imediato que oculta/inabilita o workspace. Relógio injetável e preferência local
  de inatividade, padrão 15 minutos, com opções de 1 a 240 minutos. Só eventos de
  interação local reiniciam o relógio; saída de terminal/logs não conta.
- Ocultação da janela e pausa do relógio acima de 30 s solicitam bloqueio. No macOS,
  há inscrições em NSWorkspace para suspensão e desligamento. O fechamento e o
  menu Sair/Cmd+Q passam pela revisão de documentos, com opção de proteger os
  rascunhos sem salvar remotamente. Não se instalam listeners duplicados ao repetir
  a tentativa de abrir o armazenamento.
- Buffers são capturados com documento/sessão opacos; o backend obtém perfil,
  endereço, path em bytes, revisão, BOM e finais de linha a partir do recurso
  autorizado. Nenhum caminho de destino é reconstruído a partir do texto exibido.
- Sequência implementada: capturar buffers no backend; fechar o gate de acesso;
  cancelar operações e permitir que os canais enviem TERM/HUP e fechem enquanto
  SSH ainda existe; encerrar transporte/sessões; publicar blobs e índice; somente
  após sucesso eliminar chave e modelos. A espera por canais é limitada a 3 s,
  executada em worker, sem segurar locks de gerenciadores durante a espera.
- Essa ordem encerra streams antes da gravação para impedir novas saídas/ações
  durante o bloqueio. Em falha de persistência, a chave e os buffers capturados
  continuam no backend, com UI protegida e estado `lock_failed`. A tentativa de
  recuperar ou descartar pendentes exige autenticar novamente. Descarte de
  pendentes não apaga rascunhos anteriormente publicados.
- Cada rascunho é um blob XChaCha20-Poly1305 separado em `vault/drafts/<UUID>`.
  Nonce aleatório, AAD com identificação/generação do blob, escrita exclusiva e
  sync antes de publicar o índice no envelope do cofre. Diretório 0700 e arquivos
  0600 em Unix; Windows herda a ACL restrita do diretório do cofre.
- Até 32 documentos/128 MiB por captura e 64 rascunhos publicados. Blobs têm limite
  de 40 MiB serializados. Falhas/limites não descartam os buffers. A publicação da
  nova senha recriptografa os blobs referenciados antes de substituir o índice;
  falha mantém a geração publicada anterior. Blobs órfãos não referenciados são
  limpos após publicações bem-sucedidas, sem seguir symlinks.
- Rascunhos reaparecem após reinício/desbloqueio. A visualização é local e não
  depende de conectar ao servidor. Restaurar uma aba exige o perfil/endereço
  originais; mantém o texto dirty e a revisão original para comparar ao salvar.
  Não há publicação automática. A cópia protegida permanece até exclusão explícita
  ou nova proteção do mesmo rascunho.
- Estado de terminal, logs e transferências não é retomado automaticamente.
  Conteúdo em edição passa pela WebView; não há promessa de eliminar todas as
  cópias da memória ou proteção contra sistema operacional comprometido.

## Docker e Compose

- Executor SSH em canais separados, quoting POSIX centralizado, rejeição de NUL,
  IDs canônicos validados e DSL/DTOs fechados. Nenhum comando shell arbitrário
  chega da UI. O PTY interativo continua sendo o espaço explícito para comandos.
- Docker usa somente o socket Linux local padrão `unix:///var/run/docker.sock`.
  Variáveis de daemon/contexto/TLS e overrides Compose herdados são removidos do
  ambiente da CLI. Não há sudo, instalação automática nem exposição TCP do daemon.
  Sockets rootless alternativos não são configuráveis nesta entrega.
- Inventário JSON de containers, imagens e volumes, pesquisa por nome/ID/projeto,
  atualização manual, inspect detalhado e vínculos de projeto nas entradas.
  Estado e cache pertencem à sessão/categoria. A resposta de outra sessão não
  substitui a seleção atual. Inventário limitado a 5.000 entradas por categoria;
  saída de consulta limitada a 8 MiB, com quatro consultas por sessão.
- Start/stop/restart/pause/unpause/remove usam allowlist de tipo/estado e
  confirmação de uso único com validade de 120 s, vinculada à sessão, recurso,
  ação e hash da inspeção. Estado é relido antes de executar. Volume exige nome
  exato; imagem/volume em uso são recusados pelo daemon, sem force. Remover
  container não inclui volumes.
- Logs têm canal próprio, stdout/stderr distintos, sequência/geração, ACK por
  pacote, timeout de consumidor e cancelamento. Até quatro streams por sessão,
  16 no total. Consultas finitas também são registradas para cancelamento
  ordenado. Histórico da UI limitado a 32 operações e 256 KiB de saída por operação.
  Exec abre PTY diretamente em `/bin/sh` no container, sem injetar o diretório
  inicial do host. Imagens sem esse shell podem recusar a abertura.
- Docker ausente, Compose ausente, acesso negado, desconexão e falha de comando
  têm respostas próprias; arquivos e terminais permanecem independentes.
- Projetos Compose v2 são validados por diretório canônico/configuração e
  persistidos no payload criptografado, vinculados ao perfil/endereço originais.
  Limite de 128 registros. Alterar o destino de um perfil não transfere seus
  projetos para outro servidor. Esquecer registro não executa down.
- Up detached/start/stop/restart/down têm saída e resultado por operação.
  Configuração e estado observável dos serviços são comparados antes da ação;
  campos voláteis de duração do `ps` não invalidam a revisão por simples passagem
  do tempo. Há somente uma operação Compose ativa por sessão.
- Down exige confirmação com servidor/projeto e nome exato. Volumes ficam
  desmarcados por padrão; marcar exige token adicional e texto
  `REMOVER VOLUMES <projeto>`. Um token genérico de down não autoriza volumes.
- Cancelamento fecha o acompanhamento e pede encerramento da CLI. Comandos já
  aceitos pelo daemon podem produzir efeitos. Sem replay, rollback ou reconexão
  automáticos. Comparar e executar não constitui CAS transacional; concorrência
  externa ainda pode ocorrer na janela entre os comandos.

## Verificações executadas

- `npm test`: **33 aprovados**, 14 arquivos. Inclui relógio/inatividade/retomada,
  ordem do coordenador e falha antes de captura, captura dirty sem salvar,
  isolamento de inventário/categoria e restauração que chega após fechar a sessão, cancelamento de remoção de volume, segunda
  autorização Compose e checkbox de volumes desmarcado por padrão.
- `cargo test --manifest-path src-tauri/Cargo.toml`: **40 aprovados**, 11 especiais
  ignorados por padrão. Novos casos cobrem blobs após reinício/rotação, adulteração,
  geração anterior após falha de rotação, `lock_failed` com autenticação/descarte,
  associação de projetos, quoting/NUL, parsing, allowlist e tokens expirados/
  consumidos/de outro contexto. Ignorados não são contados como aprovados.
- `native_docker_compose_lifecycle`, executado à parte na VM: **aprovado**.
  Inventário e inspect; todas as ações de containers; estado obsoleto recusado;
  dependências de imagem/volume em uso; nome incorreto recusado; remoção explícita;
  logs com ACK/cancelamento; PTY no container; caminho de projeto com espaço e
  apóstrofo; up/start/stop/restart/down; conteúdo de volume retido no down padrão
  e removido somente com autorização adicional. Perfil sem acesso ao socket
  recebeu PERMISSION_DENIED e manteve SFTP. Rascunho e registro Compose retornaram
  após bloquear/desbloquear, sem escrever o rascunho no arquivo remoto.
- `native_lock_cancels_channels`: **aprovado**. PTY, follower de logs, consulta
  finita e download na fila ativos; fechamento do gate, cancelamento e limpeza
  dos registros; nenhuma publicação do download; rascunho protegido. Após nova
  conexão, `pgrep` na VM confirmou que follower e processos de teste encerraram.
- `native_docker_missing` e `native_compose_missing`: **aprovados**. PATH sem Docker
  em usuário próprio e plugin Compose temporariamente ausente na VM. Ausências
  foram distinguidas e o restante da sessão continuou disponível.
- Smoke do `.app` no macOS, com cofre sintético e VM própria: inventário real,
  iniciar container e atualização para running, logs, terminal no container,
  edição dirty, bloqueio durante esses acompanhamentos, falha **real de permissão**
  do diretório do cofre, tela `lock_failed`, correção da permissão e recuperação
  autenticada. Conteúdo retornou na visualização e aba dirty; leitura independente
  na VM comprovou arquivo remoto intacto até clicar Salvar.
- No mesmo smoke: registro de projeto por caminho com apóstrofo, up confirmado,
  stdout/stderr com resultado concluído, diálogo de down contextual com volumes
  desmarcados e cancelamento sem executar down. Saída por Cmd+Q preservou o cofre.
- Regressões nativas `native_openssh` e `native_files`: **aprovadas** novamente
  com fixtures OpenSSH/SFTP/Agent descartáveis no macOS.
- Build TypeScript/Vite, cargo check/fmt e bundle debug `.app` executados. Vite
  mantém o aviso conhecido de chunk Monaco acima de 500 kB, carregado sob demanda.
  Scripts da fixture revisados com `bash -n` e parse AST Python.

## Ambiente e limites do aceite

A VM OrbStack foi criada com `--isolated`, 2 CPUs, limite de 3 GiB RAM/12 GiB de
armazenamento, sem pastas/socket Docker do host. Chaves/cofre/usuários/containers
foram criados exclusivamente para os testes. A ponte usou apenas TCP loopback e
`orb ... nc` para o sshd interno. Nenhum daemon, Agent ou cofre pessoal foi usado. Após a validação, foram removidos
a VM criada, a ponte SSH e os arquivos temporários sintéticos.
A receita reproduzível está em [tests/fixtures/docker](../../tests/fixtures/docker/README.md).

Pendências de QA: clientes nativos Windows/Linux e respectivas ACLs/workers/CSP;
suspensão/desligamento físicos de cada SO; disco realmente cheio, interrupção
física durante troca de geração de blobs, limpeza quando o filesystem recusa
remoção e limites máximos sob pressão de memória. O teste de falha de escrita usa
injeção controlada, complementada por permissão negada real no smoke; isso não
é um teste de ENOSPC físico. UI completa com múltiplas sessões/editores, todas as
ações destrutivas e perda física de rede ainda precisa da matriz de aceite.

No Windows/Linux, proteção de suspensão usa visibilidade e detecção do intervalo
na retomada; não há notificador nativo de suspensão antecipada nesses clientes.
Mesmo com notificação macOS, não se promete terminar uma gravação antes de uma
interrupção forçada do processo/SO. Encerrar à força em `lock_failed` pode perder
os buffers ainda não publicados; a tela avisa e a saída normal permanece bloqueada.

Cancelamento tem espera limitada e não desfaz efeitos remotos. Após perda de rede,
processos/efeitos no servidor podem sobreviver. Resíduos `.part` podem exigir
limpeza manual; histórico de transferências/logs não é restaurado após bloqueio.
Rascunhos continuam vinculados à identidade original e não são removidos
implicitamente por salvar no servidor.

Não há metadados Git/remoto neste workspace; CI remota não foi disparada.
A integração com **servidor** Linux na VM não aprova o **cliente desktop** Linux.
