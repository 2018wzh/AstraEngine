// Hosted FileUtils implementation (see platform/CCFileUtils.h).
#include "platform/CCFileUtils.h"
#include <filesystem>
#include <fstream>
#include <sstream>

namespace cocos2d {

FileUtils *FileUtils::getInstance() {
    static FileUtils instance;
    return &instance;
}

bool FileUtils::isFileExist(const std::string &fullpath) const {
    std::error_code ec;
    return std::filesystem::exists(fullpath, ec);
}

std::string FileUtils::fullPathForFilename(const std::string &filename) const {
    if(isFileExist(filename))
        return filename;
    return std::string();
}

std::string FileUtils::getStringFromFile(const std::string &filename) const {
    std::error_code ec;
    if(!std::filesystem::exists(filename, ec))
        return std::string();
    std::ifstream in(filename, std::ios::binary);
    if(!in)
        return std::string();
    std::ostringstream buffer;
    buffer << in.rdbuf();
    return buffer.str();
}

} // namespace cocos2d
