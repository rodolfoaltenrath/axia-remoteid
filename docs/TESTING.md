# Instalar e testar o Axia RemotedID Alpha 2

O Axia RemotedID conecta seus certificados em nuvem da Certisign a aplicações Linux compatíveis. Esta pré-versão é destinada ao **Fedora 44 x86_64**, com sessão gráfica. O pacote contém o aplicativo compilado; você não precisa instalar Rust nem executar scripts de desenvolvimento.

**[Baixar o RPM](https://github.com/rodolfoaltenrath/axia-remoteid/releases/download/v0.1.0-alpha.2/axia-remotedid-0.1.0-0.2.alpha2.fc44.x86_64.rpm)** · **[Downloads e checksum](https://github.com/rodolfoaltenrath/axia-remoteid/releases/tag/v0.1.0-alpha.2)**

A alpha.2 usa uma interface nativa, bandeja e configuração de navegadores com resultados por perfil. O aplicativo não executa Python ou Zenity. Não instala navegadores: identifica instalações nativas compatíveis já existentes. As bibliotecas do Fedora são instaladas automaticamente pelo gerenciador de pacotes.

## Antes de começar

- Use Fedora 44 de 64 bits, com sessão gráfica. O foco de validação é GNOME.
- Tenha um navegador nativo compatível e pelo menos um perfil criado. Se nunca o utilizou, abra e feche o navegador uma vez.
- Tenha uma conta Certisign com certificado em nuvem e acesso ao aplicativo que fornece seu código temporário.
- Feche os navegadores antes de configurar a integração.

Firefox foi confirmado em acesso real na versão anterior. Os testes automatizados da alpha.2 cobrem o novo fluxo local; o uso real desta versão ainda precisa de validação. LibreWolf, Chromium e seus derivados têm os limites descritos adiante.

## Instalar o RPM

1. Baixe o arquivo `axia-remotedid-0.1.0-0.2.alpha2.fc44.x86_64.rpm` na página de downloads.
2. Abra-o no instalador de aplicativos do Fedora e confirme a instalação com sua senha do sistema.
3. Abra **Axia RemotedID Alpha** no menu de aplicativos, como seu usuário normal.

O Fedora resolve as dependências necessárias. Se preferir instalar pelo terminal, baixe também o arquivo `.rpm.sha256`, entre na pasta dos downloads e execute:

```bash
sha256sum -c axia-remotedid-0.1.0-0.2.alpha2.fc44.x86_64.rpm.sha256
sudo dnf install ./axia-remotedid-0.1.0-0.2.alpha2.fc44.x86_64.rpm
```

O RPM ainda não tem assinatura de distribuição. O checksum permite conferir se o arquivo corresponde ao publicado nesta release.

## Conectar e configurar

1. Na primeira abertura, escolha se deseja iniciar o Axia RemotedID com o sistema. Quando ativado, os próximos logins iniciam apenas a bandeja, sem janela nem item na dock.
2. Clique em **Conectar conta**, informe os dados do aplicativo Certisign e confirme o cadastro deste computador.
3. Confira a quantidade de certificados disponíveis. Se houver um cadastro local salvo, o botão será **Atualizar certificados**, reutilizando esse cadastro.
4. Com os navegadores fechados, clique em **Configurar navegadores**.
5. Selecione os navegadores e perfis desejados e confirme. Nenhum navegador será instalado.
6. Confira os resultados no painel: **Configurado**, **Ação necessária** ou **Falha**. Se apenas alguns alvos falharem, corrija a causa e repita a ação nesses alvos.
7. Reabra os navegadores configurados e acesse o serviço que oferece entrada por certificado digital.

Os resultados de configuração ficam disponíveis enquanto o painel estiver aberto. Fechar a janela inicia uma nova lista na próxima abertura; os registros já feitos nos navegadores permanecem.

**Perfis do mesmo navegador:** duas entradas Firefox representam dois perfis, não duas instalações. Na alpha.2 publicada, um perfil vazio chamado `default` pode aparecer junto do perfil principal `default-release`. Se o principal estiver **Configurado** e somente o perfil vazio mostrar “Arquivo ou diretório inexistente”, a integração do principal permanece válida: use esse perfil e não refaça o cadastro. Na seleção, deixe marcado somente o perfil utilizado. A correção para omitir perfis ainda não preparados foi acrescentada aos fontes após a alpha.2 e ainda não faz parte desse RPM.

Quando o serviço solicitar autorização, use a senha e o código temporário atual do aplicativo Certisign. O segundo fator do serviço acessado, como o Google Authenticator exigido pelo GERID, continua fazendo parte do fluxo desse serviço.

O aplicativo não precisa manter o painel aberto para o navegador usar a integração. Fechar o painel mantém a bandeja ativa. No GNOME, a extensão **AppIndicator and KStatusNotifierItem Support** permite exibir o ícone da bandeja.

## Navegadores e limites desta alpha

| Navegador ou recurso | Situação |
| --- | --- |
| Firefox nativo | Configuração de todos os perfis existentes, com seleção pelo usuário; acesso real confirmado na versão anterior |
| LibreWolf nativo | Descoberta e seleção de perfis implementadas; autenticação real pendente |
| Chromium nativo | Autenticação TLS 1.2 testada em laboratório; uso real com Certisign pendente |
| Chrome, Brave, Edge, Vivaldi e Opera nativos | Descoberta implementada; cada distribuição precisa de validação própria |
| Navegadores Flatpak, Snap e AppImage | Integração não habilitada nesta versão |
| Assinatura de PDF | Não disponível nesta versão |

A autorização pode ser reutilizada por até dez minutos no mesmo aplicativo e certificado, mantendo somente o token em memória. Senhas e códigos temporários não são armazenados. O código do celular continua mudando normalmente; não é guardado nem reutilizado pelo cache. O serviço pode exigir nova autorização antes do prazo.

## Atualizar, remover ou testar do zero

Para atualizar, instale o novo RPM pelo mesmo procedimento, encerre o Axia RemotedID pelo menu da bandeja e reinicie os navegadores. Preserve os dados da conta: uma atualização normal não exige cadastrar novamente o computador.

Para remover o aplicativo, utilize o gerenciador de aplicativos do Fedora ou:

```bash
sudo dnf remove axia-remotedid
```

A remoção do RPM preserva a identidade, o cadastro local, a carteira, as preferências e os perfis do usuário. Ela não revoga o cadastro remoto na Certisign.

Um teste totalmente limpo exige também retirar a integração anterior dos navegadores, com eles fechados, e arquivar os dados locais da conta e as preferências. Preserve uma cópia privada: remover a identidade local faz o próximo login oferecer outro cadastro, sem apagar o cadastro remoto anterior. Não apague as bases de certificados ou o perfil inteiro do navegador para fazer esse teste.

## Alternativa: pacote compactado

A release também oferece `axia-remotedid-0.1.0-alpha.2-fedora44-x86_64.tar.gz`. Essa alternativa instala somente no seu usuário e exige previamente as bibliotecas de execução:

```bash
sudo dnf install gtk4 nss openssl-libs libgcc zlib ca-certificates
sha256sum -c axia-remotedid-0.1.0-alpha.2-fedora44-x86_64.tar.gz.sha256
tar -xzf axia-remotedid-0.1.0-alpha.2-fedora44-x86_64.tar.gz
cd axia-remotedid
./axia-remotedid
```

No painel, escolha **Instalar para este usuário** e depois conecte a conta e configure os navegadores. Não há instalador Python ou shell nesse pacote. Escolha uma das formas de instalação para o teste; não é necessário instalar o RPM e o pacote compactado juntos.

## O que testar e como relatar

Confira a abertura pelo menu, a escolha de inicialização automática, o início silencioso na bandeja, a conexão da conta, a atualização dos certificados e a seleção dos perfis. Verifique também o cancelamento, a orientação com navegador aberto, os resultados parciais e a repetição da configuração. Depois, teste o serviço desejado com certificado e suas etapas de autorização.

Ao relatar um problema, informe a versão do Fedora, navegador e forma de instalação, serviço acessado, etapa e mensagem exibida. Oculte dados pessoais em capturas e caminhos locais em relatórios. Não compartilhe senha, código temporário, tokens, arquivos de identidade, carteira ou cópias privadas de recuperação.

O projeto é independente e não é um produto oficial da Certisign. Detalhes técnicos estão no [guia de desenvolvimento](DEVELOPMENT.md).
