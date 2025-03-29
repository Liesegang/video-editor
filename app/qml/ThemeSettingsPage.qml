import QtQuick 6.8
import QtQuick.Controls 6.8
import QtQuick.Layouts 6.8
import QtQuick.Dialogs 6.8

Rectangle {
  id: themeSettings
  color: Theme.backgroundColor

  ColumnLayout {
    anchors.fill: parent
    anchors.margins: 20
    spacing: 20

    Label {
      text: "テーマ設定"
      font.pixelSize: 24
      font.bold: true
      color: Theme.textColor
    }

    GroupBox {
      Layout.fillWidth: true
      title: "テーマ選択"
      background: Rectangle {
        color: Theme.surfaceColor
        border.color: Theme.borderColor
        border.width: 1
        radius: 4
      }

      ColumnLayout {
        anchors.fill: parent
        spacing: 10

        RadioButton {
          id: lightThemeRadio
          text: "ライトテーマ"
          checked: Theme.currentTheme === Theme.LIGHT_THEME
          onClicked: Theme.setTheme(Theme.LIGHT_THEME)
        }

        RadioButton {
          id: darkThemeRadio
          text: "ダークテーマ"
          checked: Theme.currentTheme === Theme.DARK_THEME
          onClicked: Theme.setTheme(Theme.DARK_THEME)
        }

        RadioButton {
          id: customThemeRadio
          text: "カスタムテーマ"
          checked: Theme.currentTheme === Theme.CUSTOM_THEME
          onClicked: Theme.setTheme(Theme.CUSTOM_THEME)
        }
      }
    }

    GroupBox {
      Layout.fillWidth: true
      title: "カスタムカラー"
      enabled: Theme.currentTheme === Theme.CUSTOM_THEME
      background: Rectangle {
        color: Theme.surfaceColor
        border.color: Theme.borderColor
        border.width: 1
        radius: 4
        opacity: parent.enabled ? 1.0 : 0.5
      }

      GridLayout {
        anchors.fill: parent
        columns: 3
        rowSpacing: 10
        columnSpacing: 10

        // プライマリカラー
        Label {
          text: "プライマリカラー:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.primary
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: primaryColorDialog.open()
          }
        }

        ColorDialog {
          id: primaryColorDialog
          title: "プライマリカラーを選択"
          color: Theme.customColors.primary
          onAccepted: {
            Theme.setCustomColor("primary", color)
          }
        }

        // セカンダリカラー
        Label {
          text: "セカンダリカラー:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.secondary
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: secondaryColorDialog.open()
          }
        }

        ColorDialog {
          id: secondaryColorDialog
          title: "セカンダリカラーを選択"
          color: Theme.customColors.secondary
          onAccepted: {
            Theme.setCustomColor("secondary", color)
          }
        }

        // 背景色
        Label {
          text: "背景色:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.background
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: backgroundColorDialog.open()
          }
        }

        ColorDialog {
          id: backgroundColorDialog
          title: "背景色を選択"
          color: Theme.customColors.background
          onAccepted: {
            Theme.setCustomColor("background", color)
          }
        }

        // サーフェス色
        Label {
          text: "サーフェス色:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.surface
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: surfaceColorDialog.open()
          }
        }

        ColorDialog {
          id: surfaceColorDialog
          title: "サーフェス色を選択"
          color: Theme.customColors.surface
          onAccepted: {
            Theme.setCustomColor("surface", color)
          }
        }

        // テキスト色
        Label {
          text: "テキスト色:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.text
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: textColorDialog.open()
          }
        }

        ColorDialog {
          id: textColorDialog
          title: "テキスト色を選択"
          color: Theme.customColors.text
          onAccepted: {
            Theme.setCustomColor("text", color)
          }
        }

        // ボーダー色
        Label {
          text: "ボーダー色:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.border
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: borderColorDialog.open()
          }
        }

        ColorDialog {
          id: borderColorDialog
          title: "ボーダー色を選択"
          color: Theme.customColors.border
          onAccepted: {
            Theme.setCustomColor("border", color)
          }
        }

        // ハイライト色
        Label {
          text: "ハイライト色:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.highlight
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: highlightColorDialog.open()
          }
        }

        ColorDialog {
          id: highlightColorDialog
          title: "ハイライト色を選択"
          color: Theme.customColors.highlight
          onAccepted: {
            Theme.setCustomColor("highlight", color)
          }
        }

        // ホバー色
        Label {
          text: "ホバー色:"
          color: Theme.textColor
        }

        Rectangle {
          width: 30
          height: 30
          color: Theme.customColors.hover
          border.color: "black"
          border.width: 1

          MouseArea {
            anchors.fill: parent
            onClicked: hoverColorDialog.open()
          }
        }

        ColorDialog {
          id: hoverColorDialog
          title: "ホバー色を選択"
          color: Theme.customColors.hover
          onAccepted: {
            Theme.setCustomColor("hover", color)
          }
        }
      }
    }

    // プレビュー
    GroupBox {
      Layout.fillWidth: true
      title: "プレビュー"
      background: Rectangle {
        color: Theme.surfaceColor
        border.color: Theme.borderColor
        border.width: 1
        radius: 4
      }

      ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Rectangle {
          Layout.fillWidth: true
          height: 100
          color: Theme.backgroundColor
          border.color: Theme.borderColor

          RowLayout {
            anchors.fill: parent
            anchors.margins: 10
            spacing: 10

            Rectangle {
              width: 80
              height: 40
              color: Theme.primaryColor

              Text {
                anchors.centerIn: parent
                text: "Primary"
                color: "white"
              }
            }

            Rectangle {
              width: 80
              height: 40
              color: Theme.secondaryColor

              Text {
                anchors.centerIn: parent
                text: "Secondary"
                color: "white"
              }
            }

            Text {
              text: "サンプルテキスト"
              color: Theme.textColor
            }

            Rectangle {
              width: 80
              height: 40
              color: Theme.highlightColor
              border.color: Theme.borderColor

              Text {
                anchors.centerIn: parent
                text: "Highlight"
                color: Theme.textColor
              }
            }
          }
        }
      }
    }

    // ボタン
    RowLayout {
      Layout.alignment: Qt.AlignRight
      spacing: 10

      Button {
        text: "リセット"
        onClicked: {
          Theme.currentTheme = Theme.LIGHT_THEME;
          lightThemeRadio.checked = true;
        }
      }

      Button {
        text: "保存"
        highlighted: true
        onClicked: {
          Theme.saveTheme();
        }
      }
    }

    // スペーサー
    Item {
      Layout.fillHeight: true
    }
  }
}