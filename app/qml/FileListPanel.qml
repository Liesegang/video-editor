import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import Qt.labs.folderlistmodel 2.15

Rectangle {
  id: fileListPanel
  color: Theme.backgroundColor

  // Properties
  property string currentFolder: "C:/"
  property string selectedFile: ""
  property var sortColumn: "fileName"
  property bool sortAscending: true

  // Signals
  signal fileSelected(string filePath)

  // Theme change連携
  Connections {
    target: Theme
    function onThemeChanged() {
      // テーマ変更時に更新が必要な場合
      listView.forceLayout();
    }
  }

  // Model for folder contents
  FolderListModel {
    id: folderModel
    folder: "file:///" + currentFolder
    showDirsFirst: true
    sortField: {
      switch(sortColumn) {
        case "fileName": return FolderListModel.Name;
        case "fileSize": return FolderListModel.Size;
        case "fileType": return FolderListModel.Type;
        case "fileDate": return FolderListModel.LastModified;
        default: return FolderListModel.Name;
      }
    }
    sortReversed: !sortAscending
    nameFilters: ["*"]
  }

  function formatFileSize(size) {
    if (size < 1024)
      return size + " B";
    else if (size < 1024 * 1024)
      return Math.round(size / 1024) + " KB";
    else if (size < 1024 * 1024 * 1024)
      return Math.round(size / (1024 * 1024) * 10) / 10 + " MB";
    else
      return Math.round(size / (1024 * 1024 * 1024) * 10) / 10 + " GB";
  }

  function getFileType(fileName) {
    if (fileName === "..") return "Folder";
    if (folderModel.isFolder(folderModel.indexOf(fileName))) return "Folder";
    var extension = fileName.substr(fileName.lastIndexOf('.') + 1);

    switch(extension.toLowerCase()) {
      case "txt": return "Text Document";
      case "doc":
      case "docx": return "Word Document";
      case "xls":
      case "xlsx": return "Excel Spreadsheet";
      case "pdf": return "PDF Document";
      case "jpg":
      case "jpeg":
      case "png":
      case "gif": return "Image";
      case "mp3":
      case "wav": return "Audio File";
      case "mp4":
      case "avi": return "Video File";
      case "qml": return "QML File";
      default: return extension.toUpperCase() + " File";
    }
  }

  ColumnLayout {
    anchors.fill: parent
    anchors.margins: 1
    spacing: 0

    // Path bar
    Rectangle {
      Layout.fillWidth: true
      height: 30
      color: Theme.surfaceColor
      border.color: Theme.borderColor
      border.width: 1

      RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 5
        anchors.rightMargin: 5

        Image {
          Layout.preferredWidth: 16
          Layout.preferredHeight: 16
          source: "qrc:///icons/folder_icon.png"
        }

        TextField {
          id: pathField
          Layout.fillWidth: true
          text: currentFolder
          selectByMouse: true

          background: Rectangle {
            color: "white"
            border.color: Theme.borderColor
          }

          onAccepted: {
            currentFolder = text;
          }
        }

        Button {
          text: "↑"
          implicitWidth: 30
          implicitHeight: 22

          onClicked: {
            var parts = currentFolder.split('/');
            if (parts.length > 1) {
              parts.pop();
              if (parts.length > 0 && parts[parts.length-1] === "")
                parts.pop();
              currentFolder = parts.join('/') + '/';
            }
          }
        }

        Button {
          text: "⟳"
          implicitWidth: 30
          implicitHeight: 22

          onClicked: {
            folderModel.folder = "file:///";
            folderModel.folder = "file:///" + currentFolder;
          }
        }
      }
    }

    // Header
    Rectangle {
      Layout.fillWidth: true
      height: 25
      color: Theme.surfaceColor
      border.color: Theme.borderColor
      border.width: 1

      RowLayout {
        anchors.fill: parent
        spacing: 0

        // Name column header
        Rectangle {
          Layout.preferredWidth: fileListPanel.width * 0.4
          Layout.fillHeight: true
          color: "transparent"
          border.color: Theme.borderColor
          border.width: 0

          Text {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: 10
            text: "Name"
            font.bold: sortColumn === "fileName"
            color: Theme.textColor
          }

          Text {
            visible: sortColumn === "fileName"
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            anchors.rightMargin: 5
            text: sortAscending ? "▲" : "▼"
            color: Theme.textColor
          }

          MouseArea {
            anchors.fill: parent
            onClicked: {
              if (sortColumn === "fileName")
                sortAscending = !sortAscending;
              else {
                sortColumn = "fileName";
                sortAscending = true;
              }
            }
          }
        }

        // 他の列ヘッダー（サイズ、タイプ、日付）も同様に更新
        // ...（省略）...
      }
    }

    // File list
    Rectangle {
      Layout.fillWidth: true
      Layout.fillHeight: true
      color: Theme.backgroundColor
      border.color: Theme.borderColor
      border.width: 1

      ListView {
        id: listView
        anchors.fill: parent
        anchors.margins: 1
        clip: true
        focus: true

        model: folderModel

        delegate: Rectangle {
          id: fileItem
          width: listView.width
          height: 24
          color: ListView.isCurrentItem ? Theme.highlightColor :
            (mouseArea.containsMouse ? Theme.hoverColor : Theme.backgroundColor)

          RowLayout {
            anchors.fill: parent
            spacing: 0

            // ファイル名の列
            Rectangle {
              Layout.preferredWidth: fileListPanel.width * 0.4
              Layout.fillHeight: true
              color: "transparent"

              RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 5
                spacing: 5

                Image {
                  Layout.preferredWidth: 16
                  Layout.preferredHeight: 16
                  source: {
                    if (folderModel.isFolder(index))
                      return "qrc:///icons/folder_icon.png";

                    var extension = fileName.substr(fileName.lastIndexOf('.') + 1).toLowerCase();
                    if (["jpg", "jpeg", "png", "gif"].indexOf(extension) >= 0)
                      return "qrc:///icons/image_icon.png";
                    else if (["doc", "docx", "txt"].indexOf(extension) >= 0)
                      return "qrc:///icons/document_icon.png";
                    else if (extension === "qml")
                      return "qrc:///icons/qml_icon.png";
                    else
                      return "qrc:///icons/file_icon.png";
                  }
                }

                Text {
                  Layout.fillWidth: true
                  text: fileName
                  elide: Text.ElideRight
                  color: Theme.textColor
                  clip: true
                }
              }
            }

            // サイズの列
            Rectangle {
              Layout.preferredWidth: fileListPanel.width * 0.15
              Layout.fillHeight: true
              color: "transparent"
              clip: true

              Text {
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: 10
                horizontalAlignment: Text.AlignRight
                text: folderModel.isFolder(index) ? "" : formatFileSize(fileSize)
                elide: Text.ElideRight
                color: Theme.textColor
              }
            }

            // 他の列も同様に更新
            // ...（省略）...
          }

          MouseArea {
            id: mouseArea
            anchors.fill: parent
            hoverEnabled: true

            onClicked: {
              listView.currentIndex = index;
              selectedFile = fileName;
            }

            onDoubleClicked: {
              if (folderModel.isFolder(index)) {
                // フォルダに移動
                if (fileName === "..") {
                  var parts = currentFolder.split('/');
                  if (parts.length > 1) {
                    parts.pop();
                    if (parts.length > 0 && parts[parts.length-1] === "")
                      parts.pop();
                    currentFolder = parts.join('/') + '/';
                  }
                } else {
                  currentFolder = currentFolder + fileName + "/";
                }
              } else {
                // ファイルを開く
                fileListPanel.fileSelected(currentFolder + fileName);
              }
            }
          }
        }

        ScrollBar.vertical: ScrollBar {
          active: true
        }
      }
    }

    // Status bar
    Rectangle {
      Layout.fillWidth: true
      height: 24
      color: Theme.surfaceColor
      border.color: Theme.borderColor
      border.width: 1

      RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10

        Text {
          text: folderModel.count + " items"
          color: Theme.textColor
        }

        Item { Layout.fillWidth: true }

        Text {
          id: selectionInfo
          text: selectedFile ? "Selected: " + selectedFile : ""
          color: Theme.textColor
        }
      }
    }
  }
}