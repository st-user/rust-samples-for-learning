#!/bin/bash

# references
## https://askubuntu.com/questions/714458/bash-script-store-curl-output-in-variable-then-format-against-string-in-va
## https://www.cyberciti.biz/faq/bash-infinite-loop/
## https://opensource.com/article/17/1/getting-started-shell-scripting

echo "Press [CTRL+C] to stop.."

CLIENT_ID=`curl http://localhost:8080/join 2>/dev/null`

while :
do
	MESSAGE=`curl http://localhost:8080/chat?client_id=${CLIENT_ID} 2>/dev/null`
	
	if [ "${MESSAGE}" != "" ]; then
		echo ${MESSAGE}
	fi
done