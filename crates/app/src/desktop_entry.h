#pragma once

#include <QtCore/QString>
#include <QtGui/QGuiApplication>

namespace lune {

// cxx binds free functions; these forward to QGuiApplication's static ones.
inline void setDesktopFileName(const QString& name)
{
  QGuiApplication::setDesktopFileName(name);
}

inline QString desktopFileName()
{
  return QGuiApplication::desktopFileName();
}

}
