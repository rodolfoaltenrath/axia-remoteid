# Testar o Axia RemotedID Alpha

Este pacote contém o aplicativo compilado para **Fedora 44 x86_64**, com interface gráfica e integração com o Firefox nativo. Não é necessário instalar Rust ou compilar o projeto. Cada pessoa conecta sua própria conta Certisign; o pacote não inclui certificados ou dados de nenhuma conta.

## Requisitos

- Fedora 44 de 64 bits, com sessão gráfica. A validação foi feita no GNOME.
- Firefox instalado pelo Fedora, com um perfil já criado. Se nunca o utilizou, abra e feche o navegador uma vez.
- Conta Certisign com certificado em nuvem e acesso ao aplicativo de códigos temporários.

## Instalar pelo RPM

Baixe `axia-remotedid-0.1.0-0.1.alpha1.fc44.x86_64.rpm` e o arquivo `.rpm.sha256` que o acompanha. Na pasta dos downloads, execute:

```bash
sha256sum -c axia-remotedid-0.1.0-0.1.alpha1.fc44.x86_64.rpm.sha256
sudo dnf install ./axia-remotedid-0.1.0-0.1.alpha1.fc44.x86_64.rpm
```

O gerenciador de pacotes instala as dependências e adiciona **Axia RemotedID Alpha** ao menu. Depois, abra o aplicativo como seu usuário normal. Não é necessário instalar Rust ou executar scripts de compilação.

Para atualizar, instale o novo RPM pelo mesmo procedimento e reinicie o Firefox. Para remover o aplicativo, use `sudo dnf remove axia-remotedid`. O cadastro e as preferências da conta permanecem no seu perfil de usuário.

## Alternativa: pacote compactado

O arquivo `axia-remotedid-0.1.0-alpha.1-fedora44-x86_64.tar.gz` continua disponível para instalação apenas no seu usuário. Nesse formato, instale previamente as dependências:

```bash
sudo dnf install python3 zenity firefox nss openssl-libs libgcc zlib ca-certificates
sha256sum -c axia-remotedid-0.1.0-alpha.1-fedora44-x86_64.tar.gz.sha256
tar -xzf axia-remotedid-0.1.0-alpha.1-fedora44-x86_64.tar.gz
cd axia-remotedid
sh install.sh
```

Execute `sh install.sh` sem `sudo`. Para conferir os requisitos antes de instalar, use `sh install.sh --check`.

## Conectar sua conta

1. Abra **Axia RemotedID Alpha** pelo menu de aplicativos.
2. Escolha se deseja abrir o aplicativo ao entrar na sessão do Fedora.
3. Informe os dados do aplicativo Certisign e clique em **Conectar e cadastrar**. Essa ação cadastra este computador na sua conta.
4. Confirme que o aplicativo encontrou seus certificados.

Se este computador já estiver cadastrado, o aplicativo oferece **Atualizar certificados**. Preserve os dados da instalação ao atualizar a alpha.

## Integrar com o Firefox

No aplicativo instalado pelo RPM, a integração é oferecida após conectar sua conta. Clique em **Integrar Firefox**, com todas as janelas do navegador fechadas. Você também pode acessar **Integrar Firefox** pelo menu de contexto do atalho do aplicativo. Se houver vários perfis, selecione o desejado na janela.

Se você utilizou o pacote compactado, feche todas as janelas do Firefox e execute na pasta extraída:

```bash
sh install.sh --firefox
```

Reabra o Firefox, acesse o serviço desejado e escolha a entrada com certificado digital. Selecione seu certificado e autorize a operação na janela do Axia RemotedID com a senha e o código atual do aplicativo Certisign. O segundo fator exigido pelo serviço acessado continua sendo solicitado por ele.

O aplicativo não precisa permanecer aberto para essa integração funcionar. Ao atualizar os binários, reinicie o Firefox. A versão Flatpak do navegador ainda não foi validada.

No pacote compactado, se o instalador informar que há vários perfis, selecione explicitamente o perfil usado:

```bash
python3 firefox_pkcs11.py --profile /caminho/absoluto/do/perfil
```

## O que testar nesta alpha

- Abertura pelo menu e escolha da inicialização automática.
- Conexão da conta, cadastro e atualização dos certificados.
- Seleção do certificado e autenticação no Firefox. O acesso ao GERID/INSS já foi confirmado em uso real.
- Pedidos de autorização ao fechar e reabrir abas no mesmo processo do Firefox. O recurso de sessão de até dez minutos ainda precisa de validação no fluxo real; o serviço pode exigir nova autorização antes desse prazo.

Assinatura de PDF e compatibilidade com outros navegadores e portais ainda não estão validadas. Senha e código temporário não são armazenados no cache; somente o token de sessão permanece temporariamente em memória.

## Relatar um resultado

Informe a versão do Fedora, a versão e a forma de instalação do Firefox, o serviço acessado, a etapa em que ocorreu o problema e a mensagem exibida. Para capturas de tela, oculte dados pessoais. Senhas, códigos temporários, arquivos de identidade e tokens devem permanecer com você.

O projeto é independente e não é um produto oficial da Certisign. Para detalhes técnicos, consulte o [guia de desenvolvimento](DEVELOPMENT.md).
