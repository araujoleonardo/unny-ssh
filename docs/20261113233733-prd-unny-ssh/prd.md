# Product Requirements Document (PRD)

## Overview

**Produto:** unny-ssh. **Status:** rascunho para revisão. **Data:** 13/11/2026.

Aplicativo desktop para conectar a servidores SSH, editar arquivos remotos,
transferir conteúdo e operar Docker em um único ambiente. Destinado inicialmente
a desenvolvedores, profissionais DevOps e administradores que hoje alternam entre
terminal, cliente de arquivos e ferramentas de containers.

A primeira versão atenderá macOS, Windows e Linux. Terá conexões salvas em cofre
local protegido por senha mestra, explorador remoto lateral, editor Monaco,
terminal inferior e painel Docker. A experiência visual terá como referência o
Islands Dark Theme.

## Goals

- Permitir cadastrar, conectar, editar e salvar um arquivo remoto, transferir
  arquivos e operar um container sem sair do aplicativo.
- Conectar a um perfil salvo e com autenticação disponível em até duas ações após
  desbloquear o cofre, excetuando verificações de identidade do servidor.
- Atingir pelo menos 90% de conclusão dos fluxos principais em validação com cinco
  usuários representativos, sem orientação do avaliador.
- Aprovar os fluxos essenciais nos três sistemas operacionais antes da versão 1.0.
- Não sobrescrever arquivos com conflitos detectados nem remover dados Docker
  sem consentimento explícito nos cenários de aceite.
- Não persistir segredos em texto aberto no cofre, preferências ou logs gerados
  pela aplicação, verificado antes da distribuição.

Os indicadores serão medidos em testes de aceite e sessões de usabilidade, sem
necessidade de enviar dados de uso ou conteúdo dos servidores.

## User Stories

- Como desenvolvedor, quero selecionar um servidor salvo e conectar rapidamente
  para retomar meu trabalho.
- Como profissional DevOps, quero acessar servidores diretamente ou pela VPN já
  ativa e autenticar com senha, chave privada ou SSH Agent.
- Como desenvolvedor, quero explorar pastas, abrir arquivos em abas e salvar
  alterações sem baixar e reenviar manualmente cada arquivo.
- Como administrador, quero baixar uma pasta como ZIP e enviar arquivos locais
  para realizar manutenção e coletar artefatos.
- Como operador, quero consultar containers, imagens, volumes e projetos Compose
  e controlar seus ciclos de vida com clareza sobre o impacto de cada ação.
- Como usuário, quero proteger conexões e credenciais com uma senha mestra e
  receber avisos claros sobre falhas, conflitos e perda de conectividade.

## Core Features

### Conexões e sessões

**RF-01 — Perfis:** cadastrar, editar, duplicar e excluir conexões com nome, host,
porta, usuário, autenticação e pasta remota inicial opcional. Validar campos antes
de salvar e confirmar exclusão. Alterar um perfil não modifica uma sessão ativa.

**RF-02 — Acesso:** suportar SSH direto e por rede alcançável via VPN externa;
senha, chave privada com ou sem passphrase e SSH Agent. A chave pode ser importada
para o cofre; o modo Agent não exige copiar a chave. Nunca gravar credenciais sem
opção explícita do usuário. Informar dependência ausente e falha de autenticação.

**RF-03 — Identidade:** apresentar a impressão digital do servidor no primeiro
acesso para confirmação; bloquear conexão quando a identidade conhecida mudar,
permitindo atualização somente após confirmação explícita e contextualizada.

**RF-04 — Acesso rápido:** listar e pesquisar perfis no painel lateral; seleção e
ação Conectar ou duplo clique abrem a sessão. Exibir estados desconectado,
conectando, conectado e erro, com opção de desconectar e tentar novamente.

**RF-05 — Múltiplas sessões:** permitir conexões simultâneas em abas e indicar o
servidor ativo em arquivos, terminal e Docker. Ao trocar de sessão, preservar seu
contexto. Quedas devem sinalizar operações interrompidas e preservar edições em
memória; reconectar não repete automaticamente comandos ou gravações.

### Cofre local

**RF-06 — Persistência:** guardar todos os perfis, metadados sensíveis e segredos
salvos em uma pasta de cofre criptografada, acessada pela interface da aplicação
após desbloqueio com senha mestra. Copiar a pasta não permite ler seu conteúdo
sem a senha. A senha não deve aparecer em logs nem ser armazenada em texto aberto.

**RF-07 — Bloqueio:** criar e confirmar a senha no primeiro uso; permitir troca de
senha, bloqueio manual e bloqueio por inatividade configurável, com padrão de
15 minutos. Ao bloquear, encerrar sessões e ocultar conteúdo remoto. Se houver
edições pendentes, protegê-las no cofre antes de ocultar e oferecê-las na próxima
abertura, sem publicar no servidor automaticamente. Se a proteção falhar, avisar
explicitamente e não descartar silenciosamente o trabalho.

**RF-08 — Recuperação:** explicar no cadastro que não há recuperação da senha
mestra na primeira versão. Senha incorreta ou cofre inválido não deve causar
recriação, exclusão ou sobrescrita automática. A redefinição do cofre exige aviso
de perda de dados e confirmação.

### Arquivos e editor

**RF-09 — Explorador:** mostrar árvore lateral de pastas e arquivos do servidor
ativo, carregar subpastas ao expandir, atualizar listagem e exibir caminho atual.
Oferecer criar arquivo/pasta, renomear e excluir com confirmação; permissões
insuficientes devem produzir erro claro, sem declarar sucesso.

**RF-10 — Monaco:** abrir arquivos de texto no Monaco Editor em abas, com destaque
de sintaxe, busca, substituição, desfazer/refazer e indicador de alteração não
salva. Arquivos binários ou acima do limite de edição devem oferecer download e
explicar a restrição, sem travar a interface.

**RF-11 — Gravação:** salvar no caminho remoto, informar conclusão ou falha e
preservar conteúdo local em caso de erro. Se o arquivo remoto mudar após a
abertura, apresentar conflito com opções de recarregar, cancelar ou sobrescrever
explicitamente. Fechar abas ou sair com alterações deve oferecer salvar,
descartar ou cancelar.

**RF-12 — Download:** baixar arquivo individual ou pasta inteira como ZIP,
preservando a estrutura interna. Permitir escolher destino local, acompanhar
progresso e cancelar. Não seguir links simbólicos para fora da pasta selecionada;
informar itens ignorados e falhas, sem apresentar resultado parcial como completo.

**RF-13 — Upload:** enviar um ou vários arquivos locais para a pasta remota
selecionada por seletor ou arrastar e soltar. Exibir destino, progresso e
cancelamento; colisões exigem escolher sobrescrever, renomear ou ignorar. Informar
arquivos incompletos após interrupções e atualizar a árvore ao concluir.

### Terminal

**RF-14 — Shell:** oferecer terminal interativo inferior vinculado ao servidor
ativo, com múltiplas abas, redimensionamento, copiar/colar e sinais de interrupção.
Comandos devem executar no servidor identificado. Sessão encerrada deve ser
distinguível de uma sessão ociosa; saída do terminal não é persistida por padrão.

### Docker remoto

**RF-15 — Inventário:** disponibilizar painel com containers, imagens, volumes e
projetos Docker Compose do servidor ativo, pesquisa e atualização. Mostrar estado,
identificadores e dados relevantes, como portas, tags e vínculos. Docker ausente,
Compose indisponível ou acesso negado não impedem arquivos e terminal de funcionar.

**RF-16 — Containers:** oferecer start, stop, restart, pause, unpause, remoção,
inspeção, logs e terminal no container. Habilitar ações conforme o estado atual,
mostrar resultado e atualizar a listagem; operação negada não deve elevar
privilégios automaticamente.

**RF-17 — Imagens e volumes:** permitir inspecionar e remover imagens e volumes,
com aviso de dependências e confirmação. Recursos em uso devem produzir erro
compreensível. Remover volume exige confirmação com seu nome e aviso de perda de
dados; nenhuma ação deve remover volumes implicitamente.

**RF-18 — Compose:** permitir registrar um diretório remoto de projeto e executar
up, start, stop, restart e down, acompanhando saída e resultado. Explicar que down
remove containers e redes do projeto; exigir confirmação com servidor e projeto.
A remoção de volumes deve ser uma escolha adicional explícita, desmarcada por padrão.

## User Experience

O primeiro acesso apresenta a criação do cofre e o cadastro de conexão. Depois do
desbloqueio, o usuário seleciona um perfil, confirma a identidade quando necessário
e abre seu ambiente de trabalho.

O painel lateral terá seções identificáveis para conexões e árvore remota; o
centro terá abas Monaco; a área inferior conterá o terminal. Docker terá uma
área dedicada acessível sem perder a sessão. Painéis serão redimensionáveis e
recolhíveis, com restauração das preferências de layout.

A referência [Islands Dark da JetBrains](https://blog.jetbrains.com/platform/2025/09/islands-theme-the-new-look-coming-to-jetbrains-ides/)
orienta superfícies escuras e áreas visualmente separadas. A interpretação do
produto usará cantos arredondados, espaçamento consistente, hierarquia discreta e
destaques legíveis para foco, seleção e estado. Tokens e mockups serão definidos
no design, mantendo a identidade própria do unny-ssh.

Os fluxos essenciais devem funcionar por teclado, com foco visível, nomes
acessíveis, atalhos adequados a cada sistema e estados que não dependam apenas de
cor. Contraste mínimo de 4,5:1 para texto normal. Erros devem explicar a causa
conhecida e a próxima ação; operações longas devem manter a interface utilizável.

Ações destrutivas identificarão servidor e recurso. No bloqueio ou encerramento,
transferências interrompidas serão informadas, sem promessa de desfazer comandos
já enviados ao servidor.

## Assumptions & Dependencies

- Confirmado: macOS, Windows e Linux desde o início; VPN gerenciada externamente;
  senha, chave privada e SSH Agent; cofre local com senha mestra.
- Premissas a validar: uso individual, servidores Linux, Docker Engine e Compose;
  todos os módulos descritos fazem parte da versão 1.0.
- Servidores precisam oferecer SSH e operações de arquivos remotos; funcionalidades
  Docker dependem das ferramentas e permissões disponíveis naquele servidor.
- VPN, SSH Agent e credenciais válidas dependem do ambiente do usuário.
- Versões mínimas dos sistemas, formatos de chave suportados, tamanho máximo de
  edição e detalhes de retomada de rascunhos devem ser definidos na especificação
  e validados antes do aceite.
- A publicação será precedida por validação funcional nos três sistemas e revisão
  do comportamento do cofre; distribuição e assinatura precisam de planejamento.

## High-Level Technical Constraints

- Preservar a base Tauri 2, Rust e TypeScript; Monaco é requisito do editor.
- Usar protocolos seguros para conexão e transferência. Não exigir exposição
  pública do serviço Docker; operações ocorrem no contexto do servidor conectado.
- A chave de proteção do cofre não pode ficar disponível em texto aberto ao lado
  dos dados. Criptografia deve detectar adulterações e proteger dados em repouso.
- “Apenas a aplicação acessa” significa desbloqueio controlado pelo aplicativo e
  permissões locais restritas; não é garantia contra administrador, malware ou
  sistema operacional comprometido, especialmente durante uso desbloqueado.
- Não registrar senhas, passphrases, chaves ou conteúdo remoto em logs da aplicação.
  Arquivos exportados pelo usuário deixam a proteção do cofre e exigem aviso claro.
- Meta de resposta visual a ações locais: até 200 ms no equipamento de referência
  a definir. Latência de rede e tempo de comandos devem aparecer como progresso,
  sem bloquear a navegação. Não carregar árvores inteiras antecipadamente.

## Out of Scope

- Instalar, conectar ou configurar clientes e túneis VPN.
- Sincronização em nuvem, equipes, compartilhamento de credenciais e controle de
  acesso multiusuário.
- Recuperação de senha mestra, backup/exportação do cofre e importação de perfis
  de outros aplicativos na primeira versão.
- Kubernetes, Docker Swarm e orquestração entre servidores.
- Provisionamento de servidores, instalação automática do Docker e escalada
  automática de privilégios.
- Cliente Git completo, depuração remota, execução de extensões VS Code e paridade
  com uma IDE completa.
- Edição binária, edição colaborativa e sincronização contínua de diretórios.
- Bastion/jump host, encaminhamento de portas e protocolos além de SSH nesta versão.
