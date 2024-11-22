#!/bin/bash

# 设置默认的提交信息
DEFAULT_COMMIT_MESSAGE="fix"

# 检查是否提供了提交信息
if [ $# -eq 0 ]; then
    echo "未提供提交信息,使用默认信息: '$DEFAULT_COMMIT_MESSAGE'"
    COMMIT_MESSAGE="$DEFAULT_COMMIT_MESSAGE"
else
    COMMIT_MESSAGE="$1"
fi

# 获取提交信息
commit_message="$COMMIT_MESSAGE"

# 执行 git 操作
git add .
git commit -m "$commit_message"
git push

echo "提交完成！"
