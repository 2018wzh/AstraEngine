// Hosted stand-in for cocos2d::FileUtils (KRKR2_ASTRA_HOSTED only). Provides
// the subset of the cocos FileUtils API the ConfigManager layer uses,
// implemented against std::filesystem rooted at the process working
// directory.
#pragma once

#include <string>

namespace cocos2d {

class FileUtils {
public:
    static FileUtils *getInstance();
    bool isFileExist(const std::string &fullpath) const;
    std::string fullPathForFilename(const std::string &filename) const;
    std::string getStringFromFile(const std::string &filename) const;
};

} // namespace cocos2d
