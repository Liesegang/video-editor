# サードパーティライセンス通知

このプロジェクトは主に[MITライセンス](./LICENSE)の下で公開されていますが、以下のサードパーティコンポーネントを含んでいます。それぞれのコンポーネントは、記載されたライセンスに従います。

## Qt

- **ライセンス**: LGPL v3
- **ウェブサイト**: https://www.qt.io/
- **ライセンステキスト**: https://doc.qt.io/qt-6/lgpl.html

```
The Qt Toolkit is Copyright (C) 2023 The Qt Company Ltd. and other contributors.
Contact: https://www.qt.io/licensing/

This library is free software; you can redistribute it and/or modify it under the terms
of the GNU Lesser General Public License as published by the Free Software Foundation;
either version 3 of the License, or (at your option) any later version.

This library is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
See the GNU Lesser General Public License for more details.
```

このアプリケーションはQt Open Source版を使用しており、Qtライブラリは動的リンク（共有ライブラリ）形式で使用されています。LGPL v3ライセンスに従い、ユーザーは自分でQtライブラリをビルドして置き換えることが可能です。

### Qtのビルドと置き換え方法

Qtライブラリを自分でビルドして置き換える場合は、以下の手順に従ってください：

1. [Qt公式サイト](https://www.qt.io/download-open-source) からQt Open Source版をダウンロード
2. ビルドツールとともにインストール
3. 必要に応じてQtライブラリをカスタマイズ/ビルド
4. 生成された共有ライブラリファイル（.dll/.so/.dylib）を、このアプリケーションが使用している同名ファイルと置き換え

詳細は[Qt公式ドキュメント](https://doc.qt.io/qt-6/)を参照してください。

## Skia

- **ライセンス**: BSD 3-Clause "New" or "Revised" License
- **ウェブサイト**: https://skia.org/
- **ライセンステキスト**: https://skia.org/license/

```
Copyright (c) 2011 Google Inc. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.
  * Redistributions in binary form must reproduce the above
    copyright notice, this list of conditions and the following disclaimer
    in the documentation and/or other materials provided with the
    distribution.
  * Neither the name of Google Inc. nor the names of its
    contributors may be used to endorse or promote products derived from
    this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## FFmpeg

- **ライセンス**: LGPL 2.1 / GPL 2.0
- **ウェブサイト**: https://ffmpeg.org/
- **ライセンステキスト**: https://www.ffmpeg.org/legal.html

```
FFmpeg is licensed under the GNU Lesser General Public License (LGPL) version 2.1 
or later. However, FFmpeg incorporates several optional parts and optimizations 
that are covered by the GNU General Public License (GPL) version 2 or later. 
If those parts get used the GPL applies to all of FFmpeg.
```

## その他の依存ライブラリ

### cxx-qt

- **ライセンス**: MIT License
- **リポジトリ**: https://github.com/KDAB/cxx-qt

### serde / serde_json

- **ライセンス**: MIT License または Apache License 2.0
- **リポジトリ**: https://github.com/serde-rs/serde

### image

- **ライセンス**: MIT License
- **リポジトリ**: https://github.com/image-rs/image 